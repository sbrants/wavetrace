# Security policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| Latest [GitHub Release](https://github.com/sbrants/wavetrace/releases) | Yes |
| Microsoft Store build | Yes |
| Older releases | Best effort |

## Reporting a vulnerability

WaveTrace is a local desktop app — it does not collect accounts or sync data to the cloud.
If you find a security issue (e.g. path traversal in backup/restore, unsafe deserialization,
or a way to execute arbitrary code via a crafted file), please report it privately:

**Email:** [pub@brants.fr](mailto:pub@brants.fr)

Please include:

- A description of the issue and impact
- Steps to reproduce (OS, WaveTrace version)
- Proof-of-concept if available

Do **not** open a public GitHub issue for undisclosed security vulnerabilities.

## Response

- Acknowledgment within **7 days** when possible
- A fix or mitigation plan for confirmed issues affecting supported releases
- Credit in CHANGELOG if you would like (with your permission)

## Scope notes

- OCR misreads and incorrect game stats are **not** security issues — file those as regular [bug reports](https://github.com/sbrants/wavetrace/issues/new?template=bug_report.yml).
- Auto-update artifacts are signed; report failures to verify signatures as security-relevant.
