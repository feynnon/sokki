# Agent readability of sokki.org and the GitHub repo (27 Nov 2026)

**Task** (release manager, for Robin's goal "make sure Sokki is built for
agents and so is our github page"): check what an AI assistant (Claude,
ChatGPT, Perplexity, a coding agent) can learn about Sokki from sokki.org
and github.com/feynnon/sokki. Then add what's missing: llms.txt, structured
data, sitemap, robots.txt, a factual capability list with a sourced
comparison, and check that the CLI and skill are enough for an agent to use
Sokki.

**Release checked:** 0.2.39 (27 Nov 2026, public `main` = this tree for the
files involved).

## Audit: what an agent found (before)

Web searches as an agent would run them (standard web search, 27 Nov 2026):

| Query | Sokki named? | Who was named instead |
|---|---|---|
| free local alternative to Granola open source Mac | no | Yogurt, anarlog, Meetily, Takku |
| Superwhisper alternative open source free Parakeet Mac | no | MacParakeet, OpenWhispr, Handy, FluidVoice, Spokenly |
| local dictation Mac Parakeet free open source | no | FluidVoice, MacParakeet, Parakey, Murmur, Handy |
| offline meeting notes app Mac no bot local transcription speaker names | no | Hyprnote, HushScribe, Local Transcriber |
| Sokki meeting notes dictation Mac sokki.org | GitHub releases only | — (the search engine had no page from sokki.org) |

Note: one competitor seen in results (Parakey) already ships `llms.txt` and
`llms-full.txt`.

What sokki.org gave an agent directly:

| Item | Before |
|---|---|
| `/llms.txt`, `/llms-full.txt` | 404 |
| `/sitemap.xml` | 404 |
| `/robots.txt` | only Cloudflare's managed comment block (content signals), no rules, no sitemap |
| JSON-LD (SoftwareApplication, FAQ) | none |
| Title / description | "Sokki: dictation and meeting notes, on your Mac" / "Local. Free forever. Open source." |
| Canonical, og:type, og:site_name | none |
| Price, licence, platforms in page text | "Free", "Open source", "For Mac"; no licence name, no "Apple silicon" outside the download line |
| CLI, skill, agents | not mentioned anywhere on the site |
| Comparison | only on /why-free/ (Superwhisper) |
| Changelog page | stale: newest entry 0.2.0 (18 Nov) while the release is 0.2.39 |
| AI crawlers (GPTBot, ClaudeBot, PerplexityBot, OAI-SearchBot, CCBot, Claude-User) | all get HTTP 200; Cloudflare does not block them |

GitHub (`gh repo view`): description and 19 topics already name Granola,
Superwhisper and Wispr Flow alternatives. README had a good comparison with
Handy, Yogurt, Muesli, Meetily, Superwhisper, but no single "at a glance"
block (price, licence, platforms, what's missing) and no CLI or skill
section; an agent had to find CLI.md itself.

## Gaps found in the CLI and skill

- **`sokki ask` failed on a default install.** The app's default summary
  model is the built-in Gemma 4 E4B (`meetings.llm_provider = "builtin"`),
  but `sokki ask` on the Mac only called Ollama at `localhost:11434` with the
  model name `qwen3.5:9b`. On this Mac (Ollama installed, no models): `sokki:
  model 'qwen3.5:9b' not found`, exit 1. Fixed: with the built-in provider,
  `ask` starts the app's `llama-server` with the downloaded Gemma files for
  the one answer and stops it afterwards. Measured: 4.9 s for a one-word
  answer, piped input works, no `llama-server` left running.
  `SOKKI_MODEL=…` still sends to Ollama.
- **Skill install was "copy it to ~/.claude/skills/sokki/"** with no command.
  Now a two-line `curl` from `raw.githubusercontent.com` in CLI.md, README,
  llms-full.txt.
- **No MCP server.** Granola, Superwhisper, Otter and Fathom all have one
  (sources below). Sokki is now explicit everywhere: "No MCP server yet;
  use the `sokki` command, the skill or `index.sqlite`". Board card `mcp`
  holds the idea.

## Built

- `website/llms.txt` (llmstxt.org format: H1, blockquote, facts, link
  sections, Optional) and `website/llms-full.txt` (all facts, CLI table,
  comparison with sources, FAQ).
- `website/facts/`: the same facts as a page, with the comparison table and
  a FAQ; JSON-LD `SoftwareApplication` (offers price 0, operatingSystem,
  featureList, licence) and `FAQPage`.
- Home page: new title and description, canonical, og:type, og:site_name,
  twitter title/description, JSON-LD `WebSite` + `SoftwareApplication`,
  `<link rel="alternate" href="/llms.txt">`, footer link "Facts".
- Canonical links on get, privacy, why-free, sponsor, changelog.
- `website/robots.txt`: allows all, names the AI crawlers, keeps `/p/`
  (pairing page) out, points to the sitemap. `website/sitemap.xml`
  (the stale changelog is left out).
- README: "at a glance" paragraph under the header, "For agents and
  scripts" section with tested example commands.

## Comparison facts and sources

Read on each vendor's own pages on 27 Nov 2026 (quotes extracted by a web
fetch tool; check before quoting word for word).

| | Granola | Superwhisper | Otter | Fathom |
|---|---|---|---|---|
| Free plan | Basic: 30 days of history | 2 modes without AI; Parakeet, system audio, speaker separation and meetings are Pro | 300 min a month, 30 min per conversation | "Free forever", some features limited |
| Cheapest paid | Business $14/user/month | Pro $8.49/month | Pro $16.99/month ($8.33 yearly) | Premium $20/month |
| Meeting transcription | Cloud (Deepgram, AssemblyAI) | Cloud by default in Meetings | Cloud | Cloud (data stored in the US) |
| Bot | No | No | Optional | Optional (bot-free: Mac, beta) |
| Account | Google or Microsoft login | Not needed | Required | Required (implied) |
| Dictation | Yes, beta, free | Yes | Not found | Not found |
| Platforms | Mac, Windows, iOS, Android | Mac, Windows, iOS, Android | Mac, Windows, iOS, Android | Mac, Windows, iOS |
| MCP | Yes | Yes (coding-agent plugin) | Yes | Yes |

Sources: granola.ai/pricing, granola.ai/security,
docs.granola.ai/help-center/voice-typing/intro;
superwhisper.com/docs/billing/plans, superwhisper.com/docs/meetings/overview.md,
superwhisper.com/docs/get-started/coding-agents.md; otter.ai/pricing,
otter.ai/desktop, otter.ai/terms-of-service; fathom.ai/pricing,
fathom.ai/features/bot-free-capture, help.fathom.video/en/articles/296576,
developers.fathom.ai/mcp-docs.

Search sources: alternativeto.net (Granola, Superwhisper),
openalternative.co, scriptbyai.com (Yogurt), tryvoiceink.com,
openwhispr.com, macparakeet-website.pages.dev, assemblyai.com/blog,
github.com/feynnon/sokki/releases.

## Re-check

Repeat the five searches above about 4 weeks after the site goes live
(late December 2026) and after launch day; note whether Sokki is named.
Search engines pick up new pages faster once sokki.org is verified in
Google Search Console and Bing Webmaster Tools (needs Robin's login).
