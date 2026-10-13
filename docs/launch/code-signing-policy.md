# Code signing policy (DRAFT: publish only after SignPath approval)

> This text is a draft for the project home page. Do not publish it, and do not add the SignPath credit to the README,
> until the SignPath Foundation application has been approved.

Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

## Roles
- **Authors / committers:** Yusuf Uzun (https://github.com/YusufUzun03)
- **Reviewers:** Yusuf Uzun
- **Approvers:** Yusuf Uzun

## What is signed
Only `CouchRemote.exe` produced by the repository's GitHub Actions workflow (`.github/workflows/release.yml`) from a tagged commit. Local or manually built files are never signed.

## Privacy
This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it. CouchRemote talks only to the user's own devices on the local network (their phone and, if added, their TV). It contains no HTTP client, no telemetry, no accounts and no cloud service. When the user asks it to open a website (for example Netflix), it hands the address to the user's default browser. See `SECURITY.md` for details.
