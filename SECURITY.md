# Security

## Reporting a vulnerability

Please report security problems privately through GitHub:
**[Security → Report a vulnerability](https://github.com/labrat-0/ratlabs-mesh/security/advisories/new)**.
Don't open a public issue. You'll get a reply within a few days.

## How RatLabs Mesh is built to be safe

- **Local only.** The studio and its MCP endpoint listen on `127.0.0.1` and refuse requests whose
  `Host` or `Origin` isn't this machine, so web pages can't reach them (no DNS rebinding).
- **Your keys stay yours.** Provider keys live in `keys.env` in your config folder (mode 600 on
  Linux/macOS), are only sent to the provider they belong to, and never reach the browser or AI clients.
- **Downloads are checked.** The installers verify every archive against the release's `SHA256SUMS`.
- **Assets and archives are contained.** Market and starter-pack zips are extracted with path checks
  (no zip-slip); files are served only from inside the job and library folders.
- **AI clients can't spend silently.** Paid engines are labelled as such to every client, which is told
  to ask before spending; engine keys are entered in the studio, never through the AI.

Anything running as your user on the same machine can use the local studio and MCP endpoint, the same
as any other local app.
