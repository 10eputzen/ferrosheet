# Security policy

## Reporting a vulnerability

Please do **not** open a public issue for security problems.

Report them privately through GitHub: on the repository page go to **Security → Report a vulnerability** ([private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)). Only the maintainers can see the report.

Please include the ferrosheet version, your operating system, how to reproduce it (a sample file helps, with personal data removed) and what an attacker could do. You will get a first answer within a week.

## Scope

ferrosheet is a desktop app that opens files from anywhere, so reports about this code are in scope, for example:

- a crafted spreadsheet or csv file that crashes the app in a way that goes beyond a refused file, runs code or reads or writes files it should not
- the updater accepting a release without a valid signature, or installing something other than the signed download
- the Windows file association (`--register`) changing more than the current user's registry entries
- unsafe handling of formulas, links or paths found in a workbook

Out of scope: problems in third-party crates that are already fixed upstream (please report those to the crate), and a file that is merely slow to open.

## Supported versions

Only the latest release and `main` receive fixes.
