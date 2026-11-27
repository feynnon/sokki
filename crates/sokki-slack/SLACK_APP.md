# The Slack app "Sokki"

Client ID: `5519147795124.12698041507141`

- App ID A0CLJ17EX45, created 2026-11-24 from `slack-app-manifest.json`.
  PKCE (no client secret), token rotation, user scopes only (9, see
  `USER_SCOPES` in `src/lib.rs`; `users:read` + `users:read.email` added
  2026-11-25 for the notes mail after huddles), redirect
  `http://localhost:47821/slack`. A client id is public.
- Not publicly distributed yet: only members of the workspace that owns the
  app can connect; someone from another workspace presumably gets an error
  from Slack at sign-in (not tried) (KNOWN_ISSUES.md).
- Built into every build since 2026-11-27 (`sokki_slack::CLIENT_ID` in
  `src/lib.rs`; keep it the same as the id above). Before, releases left it
  out, showed no Slack row, and no huddle was ever posted. A build can use
  another Slack app with `SOKKI_SLACK_CLIENT_ID=<id>` at build time
  (`option_env!` in `src-tauri/src/meeting/slack.rs`);
  `scripts/test-meeting.sh SLACK=…` passes it to the test app at run time.
- Before public distribution: Slack asks for HTTPS redirect URLs for apps
  installed in other workspaces; whether `http://localhost` with PKCE counts
  is not documented. If not, switch to a `sokki://` redirect.
