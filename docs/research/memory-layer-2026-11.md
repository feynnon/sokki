# Memory layer: hidden use cases and a query layer for other tools (research, 26 Nov 2026)

**Task (Robin, 26 Nov 2026):** after the second-brain round (`second-brain-2026-11.md`): what are the use cases, above all the hidden ones; how has the field developed (Karpathy's autoresearch, agent memory, lifelogging, Garry Tan's gstack/gbrain); and should Sokki have a DB + embedding + metadata layer, exposed through the Sokki CLI, so other tools can query meetings and dictations? Research only, nothing built. Short report: https://claude.ai/artifact/WCsXj2G6iqRe5VqEjKEiZx

**Method:** five parallel web-research passes (field history, agent skills and meeting MCP servers, hidden work uses, hidden personal uses, query-layer design), checked against Sokki's code. No real meeting content was used.

**Result in short:** the thesis holds, in a different order. Build the index, metadata and CLI; ship keyword search with filters, stable ids and verified cites first; embeddings last (mainly for Sokki's own model), behind a private eval. The best-evidenced hidden uses need time, speaker and literal search, not AI. Fix first: stable `id` in the note, speaker id per transcript line, dictations readable by the CLI.

## Small factual lookups carry the strongest evidence

Ranked by evidence strength. W = work, P = personal, A = agents. **Measured** = study with numbers. **Documented** = shipped feature, law or journalism. **Anecdote** = one user's report or install counts.

| # | Use case | Evidence | Strength | What Sokki needs |
|---|---|---|---|---|
| 1 | P: **Doctor-visit folder**, re-listened and shared with one family member | Review of 33 studies: ~72% listened, ~68% shared with a caregiver, better recall; figures via a search summary ([JMIR 2018](https://jmir.org/2018/9/e11308/PDF); [PMC5519830](https://pmc.ncbi.nlm.nih.gov/articles/PMC5519830)) | Measured | Audio kept per visit; share one meeting |
| 2 | P/W: **Cue for half-remembered moments** | Aided + present 82%, unaided 70%, aided but absent 53%; no aided answer wrong; transcripts with 30–75% word errors plus search sufficed; narrowing the time window helped ([Vemuri 2004](https://www.media.mit.edu/speech/old/papers/2004/vemuri_UBI04_Audio-BasedPersonalMemoryAid.pdf)). Transcripts preferred over audio for context ([Trullemans 2018](https://beatsigner.com/publications/designing-prosthetic-memory-audio-or-transcript-that-is-the-question.pdf)) | Measured | Date filter, literal search, jump to the minute |
| 3 | W: **Decision archaeology**: why we decided, who proposed it, in what context | A year at Xerox PARC: revisiting records was sense-making, not lookup; the coordinator wrote "HA" (hear audio) in live notes to mark points to revisit ([Moran, CHI 97](https://chi1997.acm.org/proceedings/paper/tpm.html)) | Measured (one case) | Speakers; decisions with timestamp; a "revisit this" hotkey during the meeting |
| 4 | P: **Memory loss** | Lifelog review: ~80% event recall vs ~49% with a written diary, one amnesia patient ([Microsoft Research](https://www.microsoft.com/en-us/research/project/sensecam/alleviating-memory-loss/)) | Measured (n=1) | Daily review view |
| 5 | A: **Agents answering from meetings** | Granola's agent eval: 40% → 79% (action items 24 → 88%, meeting prep 32 → 87%) after passage search with per-chunk speakers and the instruction "notes are an index, not the answer" (transcript use 6% → 83%). Paraphrase by default cut unsupported quotes from 40% to 6% ([Granola](https://www.granola.ai/blog/how-we-help-your-agent-find-what-its-looking-for)) | Measured by vendor | Passage search, speaker per turn, verified quotes |
| 6 | P: **Speech-first writing** | Diary study, 12 writers, 10 days: psychological and productivity benefits ([Rambler, arXiv 2502.05612](https://arxiv.org/abs/2502.05612)) | Measured (small) | Dictations → drafts |
| 7 | P: **Ownership of the archive** | Omi's most-installed app is Google Drive export (~46.1K users), ~5× any "smart" app ([Omi Apps](https://h.omi.me/apps)). Humane deleted user data in Feb 2025 ([PetaPixel](https://petapixel.com/2025/02/19/humanes-ai-pin-is-dead-and-all-devices-will-stop-working-in-10-days)); Meta cut Limitless off in the EU in Dec 2025 ([9to5Mac](https://9to5mac.com/2025/12/05/rewind-limitless-meta-acquisition/)) | Documented + demand | Already there: plain files on the Mac |
| 8 | W/P: **Receipts**: what others promised, prices quoted, who said what | Bee's summary settled a disputed hotel bar charge ([Spyglass](https://spyglass.org/amazon-bee/)); a quoted GPU price found in seconds ([Lueker](https://moelueker.com/blog/limitless-ai-pendant-review-5-use-cases-worth-199)); Granola's CEO describes staff using it against a bad manager ([Platformer](https://www.platformer.news/granola-chris-pedregal-interview/)). A promise register found ~30 real loops among 135 flagged ([Mateev](https://benmateev.com/writing/every-correction-becomes-a-rule/)): expect noise | Anecdote | Speaker ≠ me (mic vs system channel), date, person |
| 9 | W: **Private self-coaching**: talk share, question spread, longest monologue, fillers, pace | Teams Speaker Coach reports are "viewable only by the user" ([Microsoft](https://support.microsoft.com/en-gb/office/improve-your-presenting-skills-with-teams-meeting-coach-b9dad791-9f8e-4960-a029-38959cc50755)). Gong: weak sellers' talk share swings 54% (won) vs 64% (lost); consistency marks top sellers ([Gong Labs](https://www.gong.io/resources/labs/talk-to-listen-conversion-ratio/)) | Documented / vendor research | Turn timings; private only |
| 10 | W: **Verbatim voice of customer** | Anthropic's sales skill: "paraphrase is not voice-of-customer"; splits internal from customer speech by email domain ([customer-voice](https://github.com/anthropics/knowledge-work-plugins/blob/main/sales/skills/customer-voice/SKILL.md)) | Documented | Attendee emails, quotes with cite |
| 11 | W: **Time log per client** | Meetings take ~30.2% of professional time (~16.6 h/week); Laurel claims +28 billed minutes per lawyer per day ([ILTA](https://iltanet.org/blogs/william-grady/2026/04/16/how-ai-is-transforming-time-capture-in-law-firms)) | Vendor claim | Nothing new: start, end, participants |
| 12 | P: **Therapy re-listening** | A panic-disorder patient replays sessions; half of a past session felt new ([Vice](https://www.vice.com/en/article/recording-my-therapy-sessions-helped-ease-my-panic-disorder/)) | Anecdote | Per-session consent |

Pattern: the top uses need **time, speaker and literal search**, not AI. A first-day Limitless user missed a plain search box ([Sinnott](https://notes.iansinnott.com/blog/posts/The+Limitless+Pendant+-+First+Impressions)).

### Untested, and only Sokki holds the data

No source shows anyone doing these.
- **Spoken predictions, checked later.** "They'll sign by March" is said aloud with a date. Resurface it at the resolution date. Decision journals fail because people forget to log ([LessWrong](https://www.lesswrong.com/posts/sfNvnRhnowKz3itCi/grading-your-own-predictions-the-parts-i-couldn-t-solve)).
- **Own-voice training pairs.** Raw dictation → the cleaned text the user kept is the prompt→response pairing style tuning needs; raw speech alone fails ([OpenAI community](https://community.openai.com/t/finetune-writing-style-from-dataset/363993)).
- **FAQ from questions others keep asking me**, drafted from my longest repeated explanations.
- **Dictation as intent log.** Compare what I dictated as a plan with what meetings and git show.
- **Vocabulary self-healing.** Superwhisper proposes scanning for mis-transcribed terms ([Superwhisper](https://superwhisper.com/blog/cli-mcp)); Sokki can seed it with calendar attendee names.
- **Autoresearch on Sokki itself.** An overnight keep/revert loop tunes cleanup or summary prompts; the metric is the user's own corrections ([program.md](https://github.com/karpathy/autoresearch/blob/master/program.md) pattern).
- **Dictated spec → goal file** that a coding loop picks up.

### What sinks these uses

- **Metrics flowing up to a boss.** Microsoft stripped user names from Productivity Score after backlash, Dec 2020 ([Microsoft](https://www.microsoft.com/en-us/microsoft-365/blog/2020/12/01/our-commitment-to-privacy-in-microsoft-productivity-score/)).
- **Other people's voices.** A Bee tester's partner banned it at home; 2 of 5 people asked agreed to be recorded ([Eka](https://ekavc.substack.com/p/living-on-record-my-first-ten-days)). §201 StGB: up to three years ([gesetze-im-internet](https://www.gesetze-im-internet.de/stgb/__201.html)). Speaker-analysis tools an employer introduces need works-council consent, because they *can* monitor (§87(1) Nr. 6 BetrVG) ([CMS](https://cms.law/de/deu/legal-updates/einfuehrung-von-ki-im-unternehmen-einbindung-des-betriebsrats)). A self-installed personal tool: no source.
- **Voiceprints.** Fireflies faces Illinois BIPA suits over speaker recognition without consent or a retention schedule ([Epstein Becker Green](https://www.ebglaw.com/insights/publications/ai-meeting-assistants-and-biometric-privacy-lessons-from-the-fireflies-ai-lawsuit)). Sokki's speaker voices are the same data: publish a retention rule, allow per-person deletion.
- **False facts.** Bee turned TV dialogue and a song into "facts" about its tester ([Futurism](https://futurism.com/ai-gadget-records-life)). Guard: every item carries speaker and source.
- **The recording graveyard.** Capture without resurfacing is "organised forgetting" ([Vibe.us](https://vibe.us/blog/recording-graveyard-adhd/), vendor). Answer: proactive surfacing, as in Rhodes' Remembrance Agent ([Rhodes](https://bradleyrhodes.com/Papers/wear-ra-personaltech/index.html)).

## The thesis holds, but embeddings come last for outside agents

**Verdict:** build the DB, the extracted metadata and the CLI. Ship keyword search with filters first. Add embeddings behind `--mode hybrid`, mainly for Sokki's own model.

- **Why a layer at all.** Agents can already grep `~/Documents/Sokki`. They cannot reach dictations (SQLite table `transcription_history`, `src-tauri/src/managers/history.rs`), match a person across renamed speakers, filter by date or app without parsing YAML, or stay inside a token budget. Today's Mac `sokki` has `transcribe`, `ask` and `config`: no search, no dictations (`scripts/sokki`).
- **Token math forces passages, not notes.** A two-hour meeting passed through the model twice costs ~50k tokens ([Anthropic](https://www.anthropic.com/engineering/code-execution-with-mcp)). Claude Code caps one tool response at 25k ([Anthropic](https://www.anthropic.com/engineering/writing-tools-for-agents)). One meeting hour is 60–80k characters ([Granola](https://www.granola.ai/blog/how-we-help-your-agent-find-what-its-looking-for)).
- **Grep matches or beats vectors for looping agents.**
  - 116 LongMemEval questions in Claude Code, Codex and Gemini CLI: grep "generally yields higher accuracy"; the harness changes scores strongly ([arXiv 2605.15184](https://arxiv.org/abs/2605.15184)). Example via a secondary summary: inline grep 93.1% vs programmatic vector 67.2% ([dev.to](https://dev.to/pueding/is-grep-all-you-need-grep-vs-vector-retrieval-for-agentic-search-534k)).
  - gbrain's own eval: a grep agent over the Markdown 67.4% vs gbrain's hybrid search 55.2% on 10M-token histories ([proof.md](https://github.com/garrytan/gbrain/blob/master/docs/proof.md)).
  - Letta: a plain agent with file tools 74.0% vs Mem0 68.5% on LoCoMo ([Letta](https://www.letta.com/blog/benchmarking-ai-agent-memory)).
  - Claude Code dropped its RAG index; the evidence was "internal vibes" ([OfficeChai](https://officechai.com/ai/claude-researcher-explains-how-agentic-search-performed-better-than-rag-for-code-generation/)).
- **Where vectors earn their keep.** Gemma 4 E4B can't run a search loop: 42.2% on tau2 ([model card](https://huggingface.co/google/gemma-4-E4B-it)). It needs one fixed hybrid retrieval. Vectors also cover paraphrases and German↔English queries. A private eval decides.
- **Metadata as search keys, not answers.** Indexing items under facts extracted from them: +9.4% recall, +5.4% accuracy. Returning summaries instead of raw text lowered accuracy. An 8B model extracting time ranges hurt recall ([LongMemEval](https://arxiv.org/html/2410.10813v2)). So "last week" becomes `--since`, resolved in code or by the calling agent.
- **Gemma's extraction is incomplete, not inventive.** Sokki's blind eval (4 meetings × 2 runs): 27/36 missing topics or tasks, padded decision lists, owner mix-ups, never an invented task (`docs/research/meeting-summaries-2026-11.md`). Every item needs a quote that code verifies.
- **Verify citations in code.** Even random citations raise trust ([arXiv 2501.01303](https://arxiv.org/abs/2501.01303)).
- **Skip** sentiment, intent and relation triples. Triple F1 is 0.55–0.73 even at 7–8B ([Graph Praxis](https://medium.com/graph-praxis/building-knowledge-graphs-with-local-llms-from-benchmarking-to-fine-tuning-ed572268aa3f)). Vendor topic detection is English-only ([Deepgram](https://developers.deepgram.com/docs/topic-detection)).

## The layer: one rebuildable index, six commands, one MCP mirror

### Fix the data first

These are cheap, and everything else depends on them.
- A stable `id:` in the note frontmatter (the folder name, e.g. `2026-03-02_10-00-00`).
- A speaker id per transcript line, or a sidecar `segments.jsonl`.
- Tasks as `- [ ] **Owner**: task <!-- t=12:04 -->`, so Obsidian edits flow back into the index.

### Store

- **Truth stays put:** the Markdown notes and `history.db`. One derived `index.sqlite`, rebuildable, every row keyed by source id. Deleting a meeting deletes its rows in one transaction, with `secure_delete`. gbrain uses the same rule: the database is "an index rebuildable from" the Markdown ([gbrain](https://github.com/garrytan/gbrain)).
- **Tables:**
  - `sources`: meeting, dictation, own notes.
  - `segments`: one speaker turn = one citable unit, split at ~120 words.
  - `chunks`: 300–500-token windows of turns, for BM25 context and later vectors.
  - `people`, `source_people`.
  - `items`: task, decision or question, each with `evidence_segment_id` and a verified quote.
  - `mentions`.
- **Changes over time:** `valid_from`/`invalid_at` on items, Graphiti-style ([Graphiti](https://mintlify.com/getzep/graphiti/concepts/temporal-model)). A later meeting that closes a task invalidates the row instead of deleting it. `--as-of DATE` reads the past.
- **German compounds:** FTS5 unicode61 plus a trigram table, so "Vertrag" finds "Rahmenvertrag". Trigram can't match under three characters ([SQLite FTS5](https://sqlite.org/fts5.html)). Measure both.

### Metadata in three tiers

| Tier | Fields | Made by | Trust |
|---|---|---|---|
| 0 | id, start/end, duration, app; calendar title and attendee emails; language; speaker, person id, name confirmed, talk time; **mic vs system channel (me vs others)**; has_audio. Per turn: start/end ms, speaker. Per dictation: time, target app, raw and cleaned text | Code | ~100% |
| 1 | decisions, tasks with owner (already in the summary call, `crates/meeting-core/src/summary.rs`), plus `due_text`, `evidence_ts`, `evidence_quote`; owner → person id and due → date in code | Gemma, checked by code | Pointer only |
| 2 | meeting type from a closed list, 3–7 key phrases, open questions | Gemma | After eval |

### CLI

```
sokki search QUERY [--person P] [--since 14d|DATE] [--until DATE]
                   [--kind meeting|dictation|notes] [--app Zoom]
                   [--section transcript|tasks|decisions] [--mode lex|hybrid]
                   [-n 10] [--max-tokens 4000] [--cursor C] [--json]
sokki get ID[@12:04] [--window 2m] [--json]
sokki meetings [--person P] [--since …] [--json]   # metadata only
sokki tasks [--owner me|NAME] [--status open] [--json]
sokki decisions [--since …] [--query …] [--json]
sokki people [--json]
sokki index status|rebuild
```

- **Output:** `{query, filters_applied, results[], next_cursor, truncated, est_tokens}`. Each result carries `cite` (`m:2026-03-02_10-00-00@12:04`), speaker, person id, whether the name is confirmed, and the turn text.
- **Defaults:** ~4k tokens per call. Unknown flags fail loudly; qmd silently ignores them and returns unscoped results ([qmd](https://github.com/tobi/qmd)). Zero results name the filter to relax.
- **Readable ids, not hashes.** Semantic ids "significantly improved" agent precision ([Anthropic](https://www.anthropic.com/engineering/writing-tools-for-agents)).
- **One Rust engine.** The Mac bash wrapper calls it; the Linux binary shares it.

### MCP and skill

- `sokki mcp`: read-only, stdio, at most five tools (`sokki_search`, `sokki_get`, `sokki_meetings`, `sokki_tasks`, `sokki_status`). Five servers with 58 tools took ~55K tokens ([Anthropic](https://www.anthropic.com/engineering/advanced-tool-use)). A CLI skill costs "a few dozen" tokens until used ([Willison](https://simonwillison.net/2025/Oct/16/claude-skills/)). The skill is the main path; MCP serves hosts without a shell.
- **Copy the vendor shape agents already know:** meeting search with filters, passage search with timestamps, transcript fetch around a passage ([Granola MCP](https://docs.granola.ai/help-center/sharing/integrations/mcp); [Fireflies MCP](https://docs.fireflies.ai/mcp-tools/overview)).
- **Copy Granola's measured instructions:** "notes are an index; the transcript is the record"; paraphrase unless asked to quote.
- **No natural-language "ask Sokki" tool by default.** The calling frontier model reads better. A local answer mode (Gemma answers, returns only the answer and its cites) is the privacy option.

### Feeding gstack and gbrain without changing them

- **How CEO review sees Slack.** Eight gstack skills, `/plan-ceo-review` among them, run "Extract 2-4 keywords … `gbrain search` … Read the top 3 results" ([gbrain.ts](https://github.com/garrytan/gstack/blob/main/scripts/resolvers/gbrain.ts)). Slack and meeting pages join those searches only with `--federated`, off by default ([gbrain guide](https://github.com/garrytan/gbrain/blob/master/docs/guides/meeting-and-slack-sources.md)). CEO review's brain budget is 5 KB ([brain-cache-spec.ts](https://github.com/garrytan/gstack/blob/main/scripts/brain-cache-spec.ts)). Whole transcripts never fit; dated decisions and quotes do.
- **Slot 1:** `sokki` writes the week's decisions, tasks and attendees to `~/.gstack/retro-context.md`. `/retro` already reads it "for meeting notes, calendar events, and decisions" ([gstack CHANGELOG](https://github.com/garrytan/gstack/blob/main/CHANGELOG.md)).
- **Slot 2:** the Sokki notes folder as a gbrain source: `gbrain sources add --path`, the IngestionSource contract, or `~/.gbrain/inbox/` ([gbrain CHANGELOG](https://github.com/garrytan/gbrain/blob/master/CHANGELOG.md)).
- **Slot 3 (needs a gstack change):** meeting decisions in the `recent-decisions` digest, which CEO review uses to "flag if this plan contradicts" a past choice ([plan-ceo-review](https://github.com/garrytan/gstack/blob/main/plan-ceo-review/SKILL.md.tmpl)).
- **Caveat:** gstack's default gbrain setup sends text to a cloud embedding provider (Voyage or OpenAI) during sync (`~/.claude/skills/gstack/USING_GBRAIN_WITH_GSTACK.md`).

### Privacy and trust

- **Off by default.** One opt-in, "Let other apps and AI agents search Sokki", with scopes: summaries + tasks | + transcripts | + dictations. Granola lets admins turn off transcript reading and keep summaries ([Granola MCP](https://docs.granola.ai/help-center/sharing/integrations/mcp)).
- **Exclusions at index time:** `private: true` per meeting; a never-index app list for dictations (banking, passwords). Filtering sensitive meetings by calendar title failed in practice, because invites are misnamed ([AGI Hunt](https://agihunt.info/en/p/1a2143294dd2c19439f39bef45f)).
- **Local audit log:** time, caller, query, ids returned; no content.
- **Say it plainly:** what a cloud agent reads leaves the Mac. Claude Code keeps session transcripts in plaintext under `~/.claude/projects/` for 30 days by default ([ContextBolt](https://contextbolt.com/blog/granola-mcp/)).
- **Returned text is untrusted data.** A calendar invite with a hidden prompt made Gemini leak private meetings ([SecurityWeek](https://www.securityweek.com/weaponized-invite-enabled-calendar-data-theft-via-google-gemini/)).
- **Writes later**, and only `sokki tasks done ID`, which edits the Markdown checkbox. The typing journal never joins the CLI or MCP.

### Eval gate before vectors

- 50–100 private questions, stored outside the repo. Categories: single meeting, across meetings, time, changed tasks, "never said" (abstention), person filters, German compounds, English↔German, dictation lookups.
- Recall@5/10 per mode: unicode61, + trigram, vectors, hybrid.
- Then Claude Code end to end, three setups: CLI + skill, MCP, raw grep on the folder. Track accuracy, valid cites, tool calls, tokens.
- If raw grep ties on accuracy at higher token cost, the CLI's value is filters and budget, and vectors stay internal.

## Since the LLM Wiki, plain files and loops won; speech is still missing

| When | What happened | What it taught |
|---|---|---|
| 1998–2007 | MyLifeBits stores one life; retrieval fails; life "9/10 mundane" ([CACM](https://cacm.acm.org/news/collect-yourself-before-you-forget-yourself)) | Total capture without a purpose fails |
| 2010 | Sellen and Whittaker: five Rs (recollect, reminisce, retrieve, reflect, remember intentions) ([Microsoft Research](https://www.microsoft.com/en-us/research/?p=159460)) | Still the best frame for use cases |
| Oct 2023 | MemGPT: tiered memory like an OS ([arXiv 2310.08560](https://arxiv.org/abs/2310.08560)) | Memory as an OS problem |
| Oct 2024 | LongMemEval: assistants lose ~30% over long chat histories ([arXiv 2410.10813](https://arxiv.org/abs/2410.10813)) | Retrieval is the bottleneck |
| 2025 | Mem0, Zep and Letta each claim state of the art; Letta's plain file agent beats Mem0 ([Letta](https://www.letta.com/blog/benchmarking-ai-agent-memory)) | Simple baselines win |
| Feb–Dec 2025 | Humane dies; Amazon buys Bee; Meta buys Limitless ([TechCrunch](https://techcrunch.com/2025/02/18/humanes-ai-pin-is-dead-as-hp-buys-startups-assets-for-116m); [TechCrunch](https://techcrunch.com/2026/01/12/why-amazon-bought-bee-an-ai-wearable/); [TechCrunch](https://www.techcrunch.com/2025/12/05/meta-acquires-ai-device-startup-limitless/)) | Vendor churn kills archives |
| Jul 2025 | Ralph loop: `while :; do cat PROMPT.md \| agent; done` ([DreamHost](https://www.dreamhost.com/blog/ralph-wiggum/)) | Agent state lives in files |
| Nov 2025 | Gemini Deep Research reads Gmail, Drive and Chat ([Google](https://blog.google/products/gemini/deep-research-workspace-app-integration/)) | Agents read personal cloud data, not speech |
| Feb 2026 | Claude Code writes its own MEMORY.md ([The Decoder](https://the-decoder.com/claude-code-now-remembers-your-fixes-your-preferences-and-your-project-quirks-on-its-own/)); LoCoMo audit: 6.4% wrong gold answers, a full-context baseline (92.62%) beats a "SOTA" system ([locomo-audit](https://github.com/dial481/locomo-audit)) | Markdown wins; recall benchmarks saturate |
| 6 Mar 2026 | autoresearch: ~700 autonomous changes in two days, ~20 kept, "time to GPT-2" 11% faster ([Karpathy](https://api.fxtwitter.com/karpathy/status/2031135152349524125)) | Loop = goal file + one mutable file + cheap metric + keep/revert + log |
| 2–4 Apr 2026 | LLM Wiki: raw sources → LLM-kept Markdown wiki → schema file ([gist](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f)) | Compile knowledge, don't re-retrieve it |
| 5 Apr 2026 | gbrain created, one day after the gist; 33.6k stars by Nov ([GitHub](https://github.com/garrytan/gbrain)) | Brain daemon for every agent |
| Apr–Jun 2026 | Claude Code routines ([Claude](https://claude.com/blog/introducing-routines-in-claude-code)); ChatGPT Dreaming V3 ([OpenAI](https://openai.com/index/chatgpt-memory-dreaming)) | Scheduled agents and overnight consolidation |
| May–Sep 2026 | Blind wiki compilation fails catastrophically in 53–60% of cases ([WiCER](https://arxiv.org/abs/2605.07068)); a wiki costs ~21× more tokens per query ([Cochran](https://arxiv.org/abs/2605.18490)); a wiki wins cross-unit questions, 9.93 vs 8.14 ([arXiv 2609.25303](https://arxiv.org/abs/2609.25303)) | Compile, and keep the raw sources |
| 16 Jul 2026 | Superwhisper ships a CLI and local MCP over dictations ([Superwhisper](https://superwhisper.com/blog/cli-mcp)); Wispr Flow's MCP excludes dictation history ([Wispr Flow](https://docs.wisprflow.ai/articles/9551372685-connect-an-mcp-client-to-wispr-flow-remote-mcp-server)) | Dictation access has one precedent |
| Oct 2026 | Siri conversation recall in iOS 27.2 ([MacRumors](https://macrumors.com/guide/ios-27-siri)); Bee Frontier answers through Alexa+ ([TechCrunch](https://techcrunch.com/2026/10/15/amazons-bee-launches-two-new-personal-AI-devices-starting-at-75)) | Platforms absorb recall |
| 2 Nov 2026 | gbrain's `meetings` source, "Fireflies first" ([gbrain guide](https://github.com/garrytan/gbrain/blob/master/docs/guides/meeting-and-slack-sources.md)) | Brains take meetings from cloud recorders |
| Nov 2026 | gbrain publishes losses: pasting the whole history scores 471/500 vs gbrain 468 on LongMemEval. gbrain wins on permission isolation (0/100 leaks vs 97–100) and planted false claims (2.8% vs 10.2%) ([proof.md](https://github.com/garrytan/gbrain/blob/master/docs/proof.md)) | Memory systems win on trust, not recall |

What this means for Sokki:
- **Loops need three things from memory:** the goal, the log of what was tried, and human judgment. Four of five frontier models score below chance at pivotal research decisions ([SciΠvot](https://arxiv.org/abs/2610.21082)). Recorded team decisions with their reasons are that scarce input.
- **Loops need a cheap, trustworthy scorer.** Where an LLM judges prose, reward hacking shows up ([awesome-autoresearch](https://github.com/WecoAI/awesome-autoresearch)). Don't promise autoresearch on fuzzy goals.
- **Where loops get context today:** deep research from cloud connectors, coding loops from repo files, scheduled agents from platform memory. None reads speech locally.
- **Overnight consolidation is table stakes.** Products differ on whether it can be reviewed (OpenAI's memory summary, gbrain's morning note with keep/undo, Letta's commit messages ([Letta](https://www.letta.com/blog/context-repositories))).
- **gbrain's founding bug is Sokki's strength.** "Three meetings with the same person became three uncorrelated timeline entries" ([ORIGIN.md](https://github.com/garrytan/gbrain/blob/master/docs/ethos/ORIGIN.md)). Sokki can fix this at capture, with named speakers and calendar attendees.

## Conclusion

"Ask my meetings" is now a commodity. Granola, Fireflies, Otter, Fathom, Read AI, tl;dv, Circleback, Krisp and Wispr Flow all ship MCP servers with the same shape ([Granola](https://docs.granola.ai/help-center/sharing/integrations/mcp); [Fireflies](https://docs.fireflies.ai/mcp-tools/overview); [Circleback](https://circleback.ai/docs/support/platform/circleback-mcp); [tl;dv comparison](https://tldv.io/blog/meeting-mcp-servers/)). Sokki's assets lie elsewhere: dictations, audio kept on the Mac for checking, a mic/system split that tells "me" from "others" without a model, and no cloud vendor that can be bought and shut down. So the layer's first customer is the frontier agent, which wants grep, filters, ids and verified cites. Its second customer is Sokki's own model, which needs the vectors. The evidence also flips the expected ranking. The proven value is literal recall by time and person, which needs no model. The flashy analytics (sentiment, engagement, burnout) are where products got sued or rolled back.

Still unmeasured: trigram vs vectors on German meetings; Gemma's precision per extracted item; whether §87 BetrVG reaches a self-installed tool; whether anyone uses spoken predictions once they resurface. The eval gate settles the first two before any vector work ships.
