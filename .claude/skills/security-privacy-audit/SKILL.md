---
name: security-privacy-audit
description: Audit project changes for credentials, personal information, private files, IP addresses, Steam IDs, account identifiers, and other sensitive information before completing tasks, committing, or publishing.
---

# Security and Privacy Audit

## 1. Core rule

Never introduce, reproduce, expose, commit, or publish real sensitive information anywhere in the project. This applies to source code, configuration, README files, changelogs, commit messages, pull request descriptions, release notes, documentation, comments, tests, logs, screenshots, build artifacts, generated files, scripts, CI/CD configuration, Git patches, and Git history.

## 2. Protected information

Treat the following as sensitive by default:

- Passwords, API keys, access and refresh tokens, OAuth credentials, session cookies, private keys, SSH keys, signing keys, certificates containing private key material, database credentials, webhook secrets, bot tokens, service account credentials, recovery codes, authorization headers, credential-bearing connection strings, cloud credentials, CI/CD secrets, and encryption keys.
- Personal names when tied to private accounts, personal email addresses, telephone numbers, residential addresses, birth dates, private messages, personal documents, photographs, and information identifying a private individual.
- Sensitive public or private IP addresses, internal hostnames, internal DNS records, VPN endpoints, server addresses, database hosts, internal service URLs, network topology, firewall details, cloud resource identifiers, and deployment details.
- Steam IDs, Steam authentication tickets, Steam session identifiers, Discord IDs or tokens, private-user platform IDs, internal account IDs, customer IDs, player identifiers, device identifiers, tracking identifiers, and identifiers linking a real person to an account.
- `.env` files, private configuration, local credential stores, browser profiles and cookies, authentication caches, local databases, dumps, backups, private documents, local application state, IDE workspace files containing sensitive data, temporary files, debug dumps, memory dumps, core dumps, and private screenshots.
- Confidential business information, proprietary internal data, non-public endpoints, sensitive vulnerability details, and private information in logs, stack traces, test results, or generated output.

A number or IP address is not automatically sensitive. Evaluate context and authorization, but do not publish real values from private environments without establishing that disclosure is safe and authorized.

## 3. Mandatory pre-completion audit

Before declaring any task complete, creating a commit, publishing changes, or generating a release:

1. Inspect the full working tree, including staged, unstaged, untracked, renamed, and generated files.
2. Review the complete diff against the intended base revision.
3. Run available secret detection, such as Gitleaks, plus targeted searches for project-specific identifiers and personal information.
4. Review README, CHANGELOG, release notes, Markdown docs, example configuration, comments, logs, test fixtures, and commit messages separately.
5. Inspect the Git index to confirm private files and sensitive data are not staged.
6. When investigating a possible existing exposure, inspect relevant history, branches, and tags without rewriting history unless authorized.
7. Correct findings introduced by current changes, then rerun relevant checks and review the final diff.
8. Report checks and limitations honestly.

Automated scanners can miss sensitive information. A successful scan alone does not prove that the project is safe.

## 4. Safe placeholders

Use fictional placeholders in examples, such as:

- `YOUR_API_KEY`
- `REPLACE_WITH_TOKEN`
- `example@example.com`
- `203.0.113.10` for documentation-only IPv4 examples
- `2001:db8::1` for documentation-only IPv6 examples
- `STEAM_ID_PLACEHOLDER`
- `USER_ID_PLACEHOLDER`
- `/path/to/example/file`

Never use slightly modified versions of real secrets as placeholders. Keep real environment files out of version control and provide placeholder-only `.env.example` files when useful.

## 5. Block on suspicious findings

If a potentially sensitive value is found:

1. Stop the affected commit, publication, or release.
2. Do not copy the value elsewhere or reproduce it in terminal output, reports, documentation, or commit messages.
3. Determine whether it is real, sensitive, and authorized for publication. If uncertain, treat it as sensitive until clarified.
4. Remove unauthorized exposure from pending changes and investigate its source.
5. Rerun the relevant checks.
6. Report only the category and location, not the value itself.

Example: `BLOCKED: potential authentication token in config/example.json, line 42. The value has not been reproduced.`

Never claim a check passed if it failed, was skipped, or could not run.

## 6. Existing exposure and Git history

Distinguish between new findings, already-tracked files, untracked or ignored files, and historical exposures. Do not silently rewrite unrelated files or delete user files without authorization. `.gitignore` does not protect files already tracked. If a real credential entered Git history, treat it as compromised and recommend immediate rotation or revocation. History cleanup may also be needed, but does not replace rotation. Do not force-push or rewrite history without authorization.

## 7. Defense in depth

Where appropriate, recommend or configure:

- `.gitignore` rules for local secrets and private files
- Pre-commit secret scanning
- CI secret scanning
- Repository-hosting secret protection
- Approved secret managers or managed secret injection
- Redaction of sensitive values in logs and errors
- Synthetic test data and least-privilege credentials

Do not blindly ignore required project files or public certificates. Review existing project requirements before changing ignore rules.

## 8. Completion checklist

Before completing a task, verify each applicable item:

- [ ] Full working tree reviewed
- [ ] Final diff reviewed
- [ ] Staged changes inspected if a commit is planned
- [ ] Secret scanning performed where available
- [ ] Personal data and account identifiers checked
- [ ] IP addresses and infrastructure details checked
- [ ] Private and machine-specific files checked
- [ ] README, changelog, docs, and commit messages reviewed
- [ ] Findings resolved or explicitly reported
- [ ] Relevant scans rerun after fixes
- [ ] Limitations disclosed

Only mark a check `PASS` if it was actually performed and no unresolved finding remains. Use `NOT RUN` when skipped and `BLOCKED` when an unresolved high-risk finding remains. If a mandatory check cannot be performed, say so clearly.

## 9. Final response format

Include a concise audit summary at the end of each task:

- Diff reviewed: PASS / NOT RUN / BLOCKED
- Secret scan: PASS / NOT RUN / BLOCKED
- Personal data and identifiers: PASS / NOT RUN / BLOCKED
- IP and infrastructure details: PASS / NOT RUN / BLOCKED
- Private files: PASS / NOT RUN / BLOCKED
- README and changelog: PASS / NOT RUN / BLOCKED
- Commit contents: PASS / NOT RUN / NOT APPLICABLE / BLOCKED
- Limitations: describe any that apply

## 10. Final principle

Security and privacy checks are part of completing the task, not optional cleanup. Never expose sensitive information to provide an example, debug a problem, document a change, or create a commit. When in doubt, stop, redact, verify, and report.
