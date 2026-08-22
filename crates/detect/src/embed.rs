//! Text → vector.
//!
//! Two implementations, chosen at runtime:
//!
//! - [`HashEmbedder`] — deterministic, no model, no network, no download.
//!   Hashed character trigrams and word n-grams. This is a *lexical* matcher:
//!   it will not recognise a paraphrase that shares no words with a trigger.
//!   It exists so the gate runs on any machine and so CI is hermetic and
//!   reproducible, which the replay harness depends on.
//!
//! - `OnnxEmbedder` — BGE-small-en-v1.5 via `fastembed`, 384-dim, the real
//!   semantic gate described in `docs/02-ARCHITECTURE.md` §7. Behind the
//!   `onnx` feature because it pulls ONNX Runtime and downloads weights.
//!
//! Both emit L2-normalised vectors of [`DIM`] floats, so cosine similarity is
//! a plain dot product and the stored blob layout is identical either way.

/// Matches BGE-small-en-v1.5 so vectors from either embedder are
/// storage-compatible.
pub const DIM: usize = 384;

pub trait Embedder: Send + Sync {
    /// Stable identifier, written to `embed_model` alongside every vector.
    /// Changing it invalidates stored embeddings, which is the intent.
    fn model_id(&self) -> &str;

    fn embed(&self, text: &str) -> Vec<f32>;

    fn embed_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

/// Cosine similarity. Both inputs are expected to be L2-normalised, which
/// makes this a dot product.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    a.iter()
        .zip(b)
        .map(|(x, y)| x * y)
        .sum::<f32>()
        .clamp(-1.0, 1.0)
}

fn l2_normalise(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

// ─── Hashed lexical embedder ─────────────────────────────────────────────

/// FNV-1a. Chosen for being stable across runs, platforms, and releases —
/// `DefaultHasher` is explicitly not, and stored embeddings must outlive a
/// process.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Lowercase, strip punctuation, collapse whitespace. Contractions are split
/// so "don't" and "do not" share the token "not" — the far-end stream is
/// conversational and ASR output is inconsistent about them.
///
/// Shared with the gate's lexical comply scan (`gate::comply_containment`), so
/// that a trigger phrase tokenises identically however it is being matched.
pub fn normalize_tokens(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    let expanded = lowered
        // Irregular forms first: a blanket "n't" -> " not" turns "can't" into
        // "ca not" and "won't" into "wo not", losing the verb stem entirely.
        // "can" and "will" carry real signal here — "I can't afford that" and
        // "I won't be switching" are different objections.
        .replace("can't", "can not")
        .replace("cannot", "can not")
        .replace("won't", "will not")
        .replace("shan't", "shall not")
        .replace("n't", " not")
        .replace("'re", " are")
        .replace("'m", " am")
        .replace("'ve", " have")
        .replace("'ll", " will");
    expanded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct HashEmbedder;

impl HashEmbedder {
    pub fn new() -> Self {
        HashEmbedder
    }

    fn add(v: &mut [f32], key: &str, weight: f32) {
        let h = fnv1a(key.as_bytes());
        let idx = (h % DIM as u64) as usize;
        // Sign bit from a different part of the hash keeps unrelated features
        // from all pushing the same direction.
        let sign = if (h >> 32) & 1 == 0 { 1.0 } else { -1.0 };
        v[idx] += weight * sign;
    }
}

impl Embedder for HashEmbedder {
    fn model_id(&self) -> &str {
        // v2: contraction handling changed, so every v1 vector is stale.
        // Bumping the id is what invalidates them — `embed_model` is stored
        // beside each vector and re-embedding is keyed off it (D12).
        "hash-lexical-v2"
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0.0f32; DIM];
        let tokens = normalize_tokens(text);

        for t in &tokens {
            Self::add(&mut v, t, 1.0);
            // Character trigrams give partial credit for morphology and for
            // ASR near-misses ("interested" / "intrested").
            let chars: Vec<char> = t.chars().collect();
            if chars.len() >= 3 {
                for w in chars.windows(3) {
                    let gram: String = w.iter().collect();
                    Self::add(&mut v, &format!("#{gram}"), 0.35);
                }
            }
        }

        // Word bigrams carry most of the signal for short objections —
        // "not interested" should not look like "interested".
        for pair in tokens.windows(2) {
            Self::add(&mut v, &format!("{}_{}", pair[0], pair[1]), 1.4);
        }

        l2_normalise(&mut v);
        v
    }
}

// ─── ONNX embedder ───────────────────────────────────────────────────────

#[cfg(feature = "onnx")]
mod onnx {
    use super::{l2_normalise, Embedder, DIM};
    use std::sync::Mutex;

    /// BGE-small-en-v1.5 via `fastembed`.
    ///
    /// BGE was trained with an asymmetric retrieval prefix. Both sides here
    /// are short conversational utterances rather than query/document, so we
    /// embed both without a prefix — consistently, which is what matters.
    /// Revisit if measured precision disappoints; see `docs/DECISIONS.md` D7.
    pub struct OnnxEmbedder {
        inner: Mutex<fastembed::TextEmbedding>,
    }

    impl OnnxEmbedder {
        pub fn new() -> Result<Self, String> {
            let model = fastembed::TextEmbedding::try_new(
                fastembed::InitOptions::new(fastembed::EmbeddingModel::BGESmallENV15)
                    .with_show_download_progress(true),
            )
            .map_err(|e| e.to_string())?;
            Ok(OnnxEmbedder {
                inner: Mutex::new(model),
            })
        }
    }

    impl Embedder for OnnxEmbedder {
        fn model_id(&self) -> &str {
            "bge-small-en-v1.5"
        }

        fn embed(&self, text: &str) -> Vec<f32> {
            self.embed_batch(&[text.to_string()])
                .pop()
                .unwrap_or_else(|| vec![0.0; DIM])
        }

        fn embed_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
            let guard = match self.inner.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            match guard.embed(texts.to_vec(), None) {
                Ok(mut vecs) => {
                    for v in vecs.iter_mut() {
                        l2_normalise(v);
                    }
                    vecs
                }
                Err(_) => texts.iter().map(|_| vec![0.0; DIM]).collect(),
            }
        }
    }
}

#[cfg(feature = "onnx")]
pub use onnx::OnnxEmbedder;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_are_normalised_and_correctly_sized() {
        let e = HashEmbedder::new();
        let v = e.embed("I never filled out any form");
        assert_eq!(v.len(), DIM);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "norm was {norm}");
    }

    #[test]
    fn embedding_is_deterministic_across_calls() {
        let e = HashEmbedder::new();
        assert_eq!(e.embed("stop calling me"), e.embed("stop calling me"));
    }

    #[test]
    fn identical_text_scores_one() {
        let e = HashEmbedder::new();
        let a = e.embed("take me off your list");
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn near_paraphrase_beats_unrelated_text() {
        let e = HashEmbedder::new();
        let trigger = e.embed("take me off your list");
        let near = e.embed("please take me off of your list");
        let far = e.embed("when does your policy renew");
        assert!(
            cosine(&trigger, &near) > cosine(&trigger, &far),
            "near {} vs far {}",
            cosine(&trigger, &near),
            cosine(&trigger, &far)
        );
    }

    #[test]
    fn bigrams_separate_negation() {
        let e = HashEmbedder::new();
        let not_interested = e.embed("not interested");
        let interested = e.embed("I am interested");
        assert!(
            cosine(&not_interested, &interested) < 0.7,
            "negation collapsed: {}",
            cosine(&not_interested, &interested)
        );
    }

    #[test]
    fn contractions_normalise() {
        let e = HashEmbedder::new();
        let a = e.embed("don't call here again");
        let b = e.embed("do not call here again");
        assert!(cosine(&a, &b) > 0.9, "got {}", cosine(&a, &b));
    }

    /// A blanket `n't` -> ` not` rule turned "can't" into "ca not" and "won't"
    /// into "wo not", throwing away the verb. "I can't afford that" and "I won't
    /// be switching" are different objections and both stems matter.
    #[test]
    fn irregular_contractions_keep_their_stem() {
        assert_eq!(
            normalize_tokens("I can't afford that"),
            ["i", "can", "not", "afford", "that"]
        );
        assert_eq!(
            normalize_tokens("I won't be switching"),
            ["i", "will", "not", "be", "switching"]
        );
        assert_eq!(normalize_tokens("cannot help"), ["can", "not", "help"]);
        // The regular rule still applies.
        assert_eq!(normalize_tokens("don't call"), ["do", "not", "call"]);

        let e = HashEmbedder::new();
        let a = e.embed("I can't afford that");
        let b = e.embed("I cannot afford that");
        assert!(cosine(&a, &b) > 0.95, "got {}", cosine(&a, &b));
    }
}
