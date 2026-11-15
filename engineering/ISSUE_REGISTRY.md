# Issue Registry

**Last updated:** 2026-11-15

| ID | Sev | Subsystem | Description | Root cause | Status | Tests | Evidence |
|----|-----|-----------|-------------|------------|--------|-------|----------|
| I-01 | P0 | App/UI | Window close discarded dirty session without confirm | Close path set quit without CancelClose + modal | **Resolved** | Manual + code path | `PendingDestructive::QuitApp`; CancelClose when `session_has_user_work` |
| I-02 | P0 | Updater | Missing SHA / arbitrary URL could apply untrusted zip | Optional sha256; no host allowlist | **Resolved** | `update::tests::reject_non_github_url_and_missing_sha` | `validate_feed` + `feed_url_is_allowed` |
| I-03 | P1 | Audio | ASIO combo implied native ASIO on portable build | Label/hint understated WASAPI fallback | **Resolved** | Label strings + honesty note path | `AudioDriverKind::Asio` label/hover; browser status `ASIO → WASAPI fallback` |
| I-04 | P2 | Audio | Exclusive/ASIO status line ignored ASIO selection | Status only Exclusive/Shared | **Resolved** | Code review | browser status match includes ASIO |
| I-05 | P2 | UI | Full every-widget egui hit-test incomplete | Computer-use pass partial | **Open / accepted** | Function-test covers handlers | Documented limitation |
| I-06 | P3 | CI | Mac/Linux CI noise on unpaid/missing deps | Matrix without continue-on-error historically | **Resolved earlier** | Workflow | `continue-on-error` non-Windows |
| I-07 | P1 | CI | Function-test not in CI | Missing workflow step | **Resolved** | CI YAML | Windows `FunctionTest-Windows.ps1` step |
| I-08 | P1 | Persistence | Autosave failures silent | `let _ = save…` | **Resolved** | Code path | Banner `Autosave failed: …` |
| I-09 | P1 | MIDI | Export .mid discarded `Result` | `let _ = export_smf_clip` | **Resolved** | DockAction::StatusMessage | Success/fail banner |
| I-10 | P2 | Docs | README still said 1.1.3 after 1.1.4 ship | Docs lag | **Resolved** | README | Links to v1.1.4 |
| I-11 | P2 | Updater | PowerShell UTF-8 BOM broke latest.json | Set-Content BOM | **Resolved earlier** | BOM strip + WriteAllText no BOM | Prior release |
| I-12 | P3 | Ship | GH_TOKEN / gh keyring auth failures on some machines | Operator env | **Open / env** | N/A | Operator must use valid `gh auth` |
| I-13 | P2 | Audio | True ASIO needs cpal asio + SDK | By design portable | **Blocked** | N/A | Honesty messaging only |
| I-14 | P3 | Scope | Mac/Linux portable packaging | Out of scope | **Blocked** | Advisory CI | N-06 |
| I-15 | P2 | Docs | Version badge inconsistency | Same as I-10 | **Resolved** | README | Merged with I-10 |

## Severity counts (this finalization pass)

| Severity | Discovered | Resolved | Open/Blocked |
|----------|------------|----------|--------------|
| P0 | 2 | 2 | 0 |
| P1 | 4 | 4 | 0 |
| P2 | 6 | 4 | 2 (I-05 accepted, I-13 blocked) |
| P3 | 3 | 1 | 2 (I-12 env, I-14 blocked) |
