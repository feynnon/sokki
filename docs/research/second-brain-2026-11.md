# Second brain in Sokki (research, 26 Nov 2026)

**Task (Robin, 26 Nov 2026):** "Having a store of information, the idea of Andrej Karpathy's second brain. Having that built-in in Sokki: would be tremendous." This covers research and a concept, with nothing built yet. The full report, with every source, is at https://claude.ai/artifact/3sUpD4a6gi3yWkJcY8pd6o. The four UX versions are on the "Sokki app" canvas, page "Offen", row "Zweites Gedächtnis".

**Method:** four parallel web-research passes (Karpathy's own posts, the tools, adoption and abandonment, a local Mac stack), checked against Sokki's code and file layout. Sources are below. No real meeting content was used.

## Findings

- **What Karpathy proposed** ([post, 2 Apr 2026](https://x.com/karpathy/status/2039805659525644595); ["LLM Wiki" gist](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f)): raw sources the model never changes, plus a Markdown wiki that the LLM writes and keeps up to date, plus a schema file. It has three operations: ingest, query (answers carry citations and get filed back), and lint (a check for contradictions and stale claims). `index.md` is the catalogue and `log.md` the record, and Obsidian is the viewer. He argues it beats RAG because the wiki builds up over time and LLMs "don't get bored" of upkeep. "Second brain" is the media's label, not his words. He says nothing about privacy or local models, and he used frontier cloud models.
- **The market** settled on "ask across my meetings" with transcript citations (Granola, Otter, Fireflies), plus pages per person or company and briefs before a meeting (Granola, Tana 2026). Almost all of it runs in the cloud. Granola deletes the audio. **No tool combines meetings, dictations and your own notes in one local store.** The local tools died for business reasons: Rewind/Limitless was bought by Meta and shut down in Dec 2025, Reor was archived in Mar 2026, Khoj Cloud closed in Apr 2026, and Mem is reported to be shutting down (one source).
- **Why people quit:** the upkeep (filing, tagging, reviewing), not the storage. No public churn numbers exist. AI memories lose users when their answers can't be checked (Rewind) or when capture feels like surveillance. Microsoft Recall kept its data on the device and still failed, because the text sat in plaintext that any process could read. Citations raise trust **even when they are random** ([arXiv 2501.01303](https://arxiv.org/abs/2501.01303)), so Sokki has to check every citation itself in code.
- **A local stack on the Mac:** the Markdown files stay the truth. One rebuildable `index.sqlite` holds FTS5 keyword search plus sqlite-vec vectors, merged with RRF (the [qmd](https://github.com/tobi/qmd) design). Embeddings come from EmbeddingGemma 300M (under 200 MB quantised) or Qwen3-Embedding 0.6B, via Sokki's llama-server `--embedding`. A reranker is optional. Search over about 1,000 meetings takes under 20 ms (an estimate, not measured). The embedder and reranker add about 1 GB, which still has to be measured against the 6.4 GB cap.
- **The limit is the model.** Small models (360M–8B) ignore retrieved context, and retrieval wiped out 42–100 % of answers they had known ([arXiv 2603.11513](https://arxiv.org/abs/2603.11513)). Gemma 4 E4B scores 25.4 % on MRCR at 128k and 42.2 % on tau2. So Sokki should run one search step decided in code, hand the model ≤ 6–8k tokens of quoted evidence, have it answer by quoting, and say "found nothing" when nothing turns up. No agent loop and no GraphRAG: extracting relations is weak below 30B. Karpathy's "no RAG needed" doesn't carry over to a 4B model at 16k context.

## Sokki today (checked 26 Nov 2026)

- Meeting notes are already Markdown with YAML frontmatter in `meetings.notes_dir` (`~/Documents/Sokki`). The sections are Zusammenfassung, Entscheidungen, Aufgaben and Transkript. This already works as Karpathy's raw layer and opens in Obsidian.
- The meeting note's frontmatter has no stable meeting `id`, and the file name follows the title. The own-notes format (fixed 26 Nov 2026) gives the brain a stable anchor:
  - Raw typed notes are stored in `meetings/<id>/notes.md` in the app data folder. `<id>` is the meeting folder name, which never changes. The frontmatter holds `id`, `source: own-notes`, `started_at`, `ended_at` and `title`.
  - Each line ends with `<!-- t=hh:mm:ss -->`, its time in the recording.
  - The meeting note gets a `## Meine Notizen` section. Each top-level bullet is the user's line, copied word for word. Nested bullets are Sokki's additions.
  - Tasks are written as `- [ ] **Owner**: task`.
  - The brain should read `notes.md` for the user's own words and the meeting note for everything else.
- Audio is deleted once the note is written (`meetings.keep_audio_days` = 0). A citation can play the exact minute only when Keep audio is on; otherwise it opens the transcript line. No competitor we found can play the cited minute.

## Concept and plan

Three layers:
1. The raw files: meeting notes, own notes and dictations. The typing journal joins only with its own opt-in, encrypted, with excluded apps, and never feeds people pages, briefs or MCP.
2. A rebuildable index. Every row carries its source id, so deleting a meeting deletes its rows; SQLite runs with `secure_delete`.
3. A short wiki that cites every line: pages for people (only speakers the user confirmed, facts only) and for projects. Sokki updates it after each meeting, shows what changed with undo, and never overwrites lines the user wrote.

The UX versions (canvas) are A Ask, B Pages, C Without asking (a brief before a call, related lines while you type notes, a Friday recap), and D Voice. They can be combined.

| Phase | What | Gate |
|---|---|---|
| 0 now | Own notes in the agreed format: `meetings/<id>/notes.md` plus `## Meine Notizen`, `t=` per line, tasks `- [ ] **Owner**: task`. Still open: `id` in the meeting note's frontmatter, and a speaker id per transcript line | – |
| 1 small | Search with FTS5 across notes, own notes and dictations; jump to the minute; scope chips; CoreSpotlight donation (macOS search and Siri, on-device) | Measure what people look for; test German compound words |
| 2 | Ask (A): pick the embedder on German data, hybrid search, quoted answers, quotes checked in code | Grounded-answer rate on a private eval set (outside the repo) |
| 3 | Pages (B) and the brief before a call (C) | Facts only, with diarization confidence |
| 4 | Recap, related lines while typing, voice (D), a follow-up mail Sokki sends after opt-in | – |
| 5 | Local MCP server (read-only first, warning for cloud agents), Obsidian vault, journal as a separate opt-in | – |

Open: Gemma's memory use next to the embedder; whether people use the brief more than the ask box; whether CoreSpotlight content reaches Siri AI semantically; SQLCipher with sqlite-vec.

## Sources (main)

Karpathy: [X post](https://x.com/karpathy/status/2039805659525644595), [gist](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f), [HN discussion](https://news.ycombinator.com/item?id=47640875). Tools: [Granola chat](https://www.granola.ai/blog/chat-with-meetings-search-analyze-ai-2026), [Tana](https://tana.inc/blog/what-are-agentic-meetings-2026), [TechCrunch on Limitless](https://www.techcrunch.com/2025/12/05/meta-acquires-ai-device-startup-limitless/), [Apple Siri AI](https://www.apple.com/newsroom/2026/06/apple-introduces-siri-ai-a-profoundly-more-capable-and-personal-assistant/). Trust and quitting: [Ding et al.](https://arxiv.org/abs/2501.01303), [Epstein et al. CHI 2016](https://pmc.ncbi.nlm.nih.gov/articles/PMC5428074), [Recall plaintext](https://www.malwarebytes.com/?p=112236). Stack: [qmd](https://github.com/tobi/qmd), [EmbeddingGemma](https://huggingface.co/blog/embeddinggemma), [Qwen3-Embedding](https://arxiv.org/pdf/2506.05176), [small models and RAG](https://arxiv.org/abs/2603.11513), [Gemma 4 E4B card](https://huggingface.co/google/gemma-4-E4B-it). All other sources are in the report.
