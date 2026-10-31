# Contributing to openHLSW

Thanks for helping! Bug reports, ideas, translations and code are all welcome.
By taking part you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting bugs and ideas

- Use the [issue forms](https://github.com/DennisPohlmann/openHLSW/issues/new/choose):
  **Bug report** or **Feature request**. Search the existing issues first.
- Security problems are never reported in a public issue; see
  [SECURITY.md](SECURITY.md).
- Do not post real RCON passwords, Steam IDs, server addresses or other private
  data in issues, logs or screenshots. Use placeholders such as `203.0.113.10:27015`
  and `YOUR_RCON_PASSWORD`.

## Building and testing

You need stable Rust (version in `rust-version` of `Cargo.toml`, see
[README → Building](README.md#building) for the system packages). Before you
open a pull request, run:

```sh
cargo clippy --all-targets -- -D warnings
cargo test
```

`tools/fake_srcds.py` simulates a Source, GoldSrc or GameSpy 2 server, so you can
test queries, RCON and the server log without a real server.

CI builds and checks every pull request on all systems.

## Pull requests

- One logical change per pull request; keep unrelated clean-ups separate.
- Update `README.md` in the same pull request when a feature, setting, default
  or command changes.
- New user-visible text is English in the code (`tr!("...")`) and needs entries
  in the language files in `assets/lang/` where you can provide them; missing
  entries fall back to English.
- New third-party code, icons, fonts or data need a license that is compatible
  with GPL-3.0-or-later and a note in the README section License.
- Do not edit `CHANGELOG.md` or the version in `Cargo.toml` / `Cargo.lock`, and
  do not create tags: releases are made automatically.
- egui's painting crates in `vendor/` are patched copies; changes there follow
  [vendor/README.md](vendor/README.md).

## Commit messages

openHLSW uses [Conventional Commits](https://www.conventionalcommits.org/); the
changelog and the version are generated from them.

```
<type>(<optional scope>): <summary>
```

- Summary in English, imperative, lower case, no full stop, about 72 characters
  at most. The body explains what changes for users.
- Types: `feat` (new feature), `fix` (bug fix), `perf`, `refactor`, `docs`,
  `test`, `build`, `ci`, `chore`. Security fixes are `fix(security): ...`.
- A breaking change uses `feat!:` / `fix!:` or a `BREAKING CHANGE:` footer.

Examples: `fix: keep marked text when right-clicking a text field`,
`feat: add the Teeworlds query`.

## Translations

Each language is a JSON file in `assets/lang/` that maps the English text to
the translation (`{"Refresh": "Aktualisieren"}`). Keep the `{placeholders}`
unchanged; `cargo test` checks them.

## License

openHLSW is licensed under the
[GNU General Public License v3.0 or later](LICENSE). By submitting a
contribution you agree that it is published under this license.
