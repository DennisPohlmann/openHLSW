# Security Policy

## Supported versions

Security fixes go into the next release. Only the
[latest release](https://github.com/DennisPohlmann/openHLSW/releases/latest) is
supported; please update before you report a problem.

## Reporting a vulnerability

Please **do not open a public issue** for a security problem.

Report it privately through GitHub instead: open the
[Security tab](https://github.com/DennisPohlmann/openHLSW/security) of this
repository and click **Report a vulnerability**. Only the maintainer can read
the report.

Please include:

- the openHLSW version and your system (Windows, Linux, macOS, FreeBSD),
- what an attacker can do and under which conditions (for example a malicious
  game server answering a query, an RCON reply, an imported server list),
- steps or a small file to reproduce it.

Do not send real RCON passwords, Steam IDs, server addresses or other private
data; use placeholders such as `203.0.113.10` and `YOUR_RCON_PASSWORD`.

You get an answer within a few days. When the problem is confirmed, it is fixed
in a new release and published as a GitHub security advisory; you are credited
there unless you prefer not to be.

## Scope

openHLSW talks to game servers it does not control, so answers from servers
(queries, RCON, server log over UDP) are untrusted input. Problems such as
crashes, hangs or excessive memory use caused by such answers, leaked RCON
passwords (in files, logs or exports), or unsafe handling of imported files are
in scope.

Out of scope: problems of the game servers themselves, of the services openHLSW
uses (for example ip-api.com for locations) and attacks that need full access to
the user's account or computer.
