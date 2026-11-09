# MandateLab Hackathon — specification bundle

**Created:** 4 November 2026. **Status:** Project plan and implementation instructions; not deployed software.

## Included
- `MandateLab_PRD_v1.md`: full product requirements, smart-contract state/API, backend model, screens, stages, acceptance criteria, deployment/release criteria.
- `MandateLab_Threat_Model_and_Test_Plan.md`: attack surfaces, asset trust boundaries, formal financial invariants, mitigations, verification procedure.
- `MandateLab_Test_Cases.csv`: 34 P0/P1/P2 test cases initialized `NOT_RUN` for honest progress tracking.
- `MandateLab_Supabase_Schema.sql`: optional P1 database migration for synthetic scenarios, run history and evidence (never a replacement for chain verification).
- `MandateLab_Coding_Agent_Prompt.md`: copy-ready detailed prompt for Codex, Claude Code or equivalent, with proof-first workflow.

## Quick start
1. Read `MandateLab_PRD_v1.md`, especially Sections 0, 4, 5, 11 and 12.
2. Create an empty repository named `mandatelab`.
3. Place all four specification files under `docs/` (except prompt may stay at root).
4. Give your coding agent the full contents of `MandateLab_Coding_Agent_Prompt.md` and access to the repository.
5. Prioritize P0: Rust contract -> unit tests -> local sandbox -> real NEAR testnet authorized + unauthorized transactions.
6. Record actual public transaction proofs. Never use real funds or commit credentials.

## Important technical source notes
- NEAR Rust SDK release pinned for planning: **5.30.0**; its release notes indicate **Rust 1.95** compatibility. Confirm on implementation machine.
- Function-call access-key gas allowance is **not** a payment cap; enforce payment policy inside contract.
- `request_payment` scheduling does not equal settled payment; async callback must confirm success/failure.
- NEAR AI Cloud has an OpenAI-compatible endpoint; model privacy guarantees vary by model/provider and should only be claimed after verification.
- Official hackathon deadline: **14 November 2026, 23:59 UTC**.

Official sources are linked in the PRD and coding agent prompt.
