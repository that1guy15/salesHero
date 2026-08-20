-- 0002_escalation.sql — structural repeat-denial escalation.
--
-- docs/DECISIONS.md D8. `insurance-cold-call.yaml` carried two obligations as
-- prose an LLM was asked politely to respect:
--
--   obj_no_form         "If they deny it a second time, treat it as a removal request."
--   obj_not_interested  "One attempt only. A second 'not interested' ends the call."
--
-- Every other compliance guarantee in this project is structural — type level,
-- SQL trigger, required test. These two were text. This column makes them
-- structural: after N fires of this objection within one call, the pipeline
-- promotes the playbook's `comply` card instead of the rebuttal.
--
-- NULL means no escalation, which is the correct default for most objections.
-- Forward-only; never edit an applied migration.

ALTER TABLE objections ADD COLUMN escalate_after_repeats INTEGER;
