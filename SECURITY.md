# Security Policy

OpenKara takes security reports seriously. This file tells you how to report a vulnerability and what to expect after you report it.

## Reporting a Vulnerability

Report security issues through GitHub's private vulnerability reporting channel. Open the repository on GitHub. Go to the **Security** tab. Select **Report a vulnerability**. Submit your report there.

Do not open a public GitHub issue for a security problem. Do not publish a vulnerability in any public channel before the fix ships.

Include these details in your report:

- A description of the vulnerability.
- The steps to reproduce the issue.
- The affected version or commit.
- The impact on a user or system.
- A CVSS v3.1 score if you have one.

## Supported Versions

Only the latest release line receives security fixes. Older releases do not get backports. Upgrade to the latest release before you report an issue.

## Severity Classification

OpenKara rates vulnerabilities with CVSS v3.1. The table maps the score to a severity.

| Severity | CVSS v3.1 Score |
| -------- | --------------- |
| Critical | 9.0 - 10.0      |
| High     | 7.0 - 8.9       |
| Medium   | 4.0 - 6.9       |
| Low      | 0.1 - 3.9       |

## Response SLA

The maintainer acknowledges and fixes each report on this schedule.

| Severity | Acknowledge within | Fix within   |
| -------- | ------------------ | ------------ |
| Critical | 48 hours           | 7 days       |
| High     | 72 hours           | 30 days      |
| Medium   | 7 days             | Next release |
| Low      | 7 days             | Next release |

The maintainer tells the reporter when the fix is ready. The maintainer also tells the reporter if the report is declined and why.

## Coordinated Disclosure

OpenKara uses a 90-day disclosure window. The maintainer publishes the fix and the advisory after 90 days from the report date, or sooner if the reporter and the maintainer agree.

The maintainer credits the reporter by name unless the reporter asks to stay anonymous.

The maintainer discloses early only when all of these conditions are true:

- The vulnerability is under active exploitation.
- A fix is available.
- The reporter agrees to the early disclosure.
