# Avatar Upload — Runtime Decision Matrix

`--picture` on `agent create` / `agent update` takes a URL. To get a URL, you either (a) already have one, or (b) run `onchainos agent upload --file <path>` to get one from a local image. **The "CDN" / "upload pipeline" are implementation details — never expose these words to the user.** To the user it's simply "发一张图片给我，我帮你上传" / "send me an image, I'll handle the upload".

The right path depends on the runtime. Do not force the user down a path their environment cannot support.

| Runtime | User provides image | AI-generated image | Skip avatar |
|---|---|---|---|
| **Claude Code (desktop / IDE)** | ✓ save attachment to a temp path → `agent upload --file <path>` → take returned URL | ✓ call the image-gen tool → save to temp path → `agent upload --file <path>` → URL | ✓ omit `--picture`; backend assigns default |
| **Plain terminal / CLI chat** | ✗ no file inline — do NOT ask the user to locate a path on disk | ✓ describe the prompt to the image-gen tool (user cannot preview but the URL works) | ✓ omit `--picture` |
| **User writes the command themselves** | ✓ they pass `--picture <url>` directly | N/A | ✓ they omit `--picture` |

---

## Policy

1. **Detect the runtime first.** If the session has an attachment facility (Claude Code attachments, editor drag-drop), allow "user-provided image". If it is a terminal-only chat, do not ask the user for a path.
2. **Default to "skip".** If the user doesn't bring up avatar, do not prompt for one; create/update succeeds with a backend-assigned default image.
3. **When prompting, match the user's language and use the numbered-options pattern (`SKILL.md §Choice prompts`):**
   - Claude Code (attachments supported) — 3 options:
     - 中文：
       ```
       要设置头像吗？
         1. 发一张图片给我，我帮你上传（推荐 1:1 方图，PNG/JPEG/WebP）
         2. 用关键词让我生成一张
         3. 跳过（用默认头像）
       回复 1/2/3。
       ```
     - English:
       ```
       Want to set an avatar?
         1. Send me an image and I'll handle the upload (1:1 square recommended, PNG/JPEG/WebP)
         2. Generate one from keywords
         3. Skip (use the default avatar)
       Reply 1/2/3.
       ```
   - Terminal (no attachments) — only 2 options (local upload won't work here):
     - 中文：
       ```
       当前环境没法直接收图。你可以：
         1. 用关键词让我生成一张（推荐 1:1 方图）
         2. 跳过（用默认头像）
       回复 1 或 2。
       ```
     - English:
       ```
       I can't receive attachments in this environment. You can:
         1. Generate one from keywords (1:1 square recommended)
         2. Skip (use the default avatar)
       Reply 1 or 2.
       ```
4. **AI-generation requires explicit prompt.** Do not invent image content. Ask the user in their language: "你希望头像长什么样？几个关键词就够。" / "Describe the avatar — a few keywords is enough."
5. **Upload result is a URL — show it to the user.** Pass it to `--picture`, and render the URL verbatim in the Picture row of the confirmation card and the detail card (see `display-formats.md §Picture row rule`). Do **not** hide the URL behind "已上传" / "uploaded" or any placeholder. Do not re-upload an already-uploaded image.
6. **Aspect ratio guidance.** When the user sends a non-1:1 image, accept it and upload anyway — do not reject or demand re-crop. But when *proactively* recommending dimensions, say "1:1 方图 / 1:1 square" rather than a specific pixel size like 512×512 (the backend does not enforce 512, and pixel specifics date quickly).

---

## Claude Code flow (attachment-supported)

```
User: "帮我注册 provider，名字 Alice，顺便用这张图做头像" [attaches file]
Skill:
  1. Save attachment → /tmp/<random>.png
  2. onchainos agent upload --file /tmp/<random>.png
     ← { url: "<url>" }
  3. Run agent create with --picture "<url>"
  4. Clean up the temp file
```

After the upload succeeds, move to the next step of the flow silently — or with a one-line ack that includes the URL so the user can see what was set:
- 中文："好，头像已加好：`<url>`"
- English: "Got it, avatar set: `<url>`"

The URL **must** appear verbatim in the Picture row of the confirmation card (`display-formats.md §Picture row rule`), and in the detail card after success. Never replace it with `已上传` / `uploaded` / `CDN` or any placeholder phrase. Never mention the word "CDN" to the user.

## Claude Code flow (AI-generated)

```
User: "用一只戴眼镜的青蛙当头像"
Skill:
  1. Call image-gen tool with prompt → /tmp/<random>.png
  2. Show the generated image to the user, confirm ("这张 OK 吗？" / "Does this work?")
  3. onchainos agent upload --file /tmp/<random>.png → URL
  4. Proceed with agent create / update
```

## Terminal flow (no attachments)

```
User: "上传个头像"
Skill: (render the 2-option numbered prompt — see Policy §3, Terminal variant)
  - User replies "1" / "生成" / "generate" → ask keywords → image-gen → upload → URL (silently)
  - User replies "2" / "跳过" / "skip" → proceed without --picture
  - Anything else → re-render the numbered prompt once; never silently default.
```

## User-provided URL

If the user already hands over a URL (e.g., "用这个 twitter 头像 https://..."), trust it and pass directly as `--picture`. Do not re-download and re-upload.

---

## Validation

- **MIME type** — backend is known to accept PNG / JPEG / WebP; other types are likely rejected with a backend-originated error (exact wording is not a CLI `bail!` and may drift — do NOT hard-code). On rejection, ask the user to convert to PNG / JPEG / WebP and retry.
- **File size** — no explicit limit in the CLI; if backend rejects with a size error, ask the user to downscale (≤ 2 MB is a safe default).
- **URL shape** — must be HTTPS. On invalid shape, in the user's language:
  - 中文："头像链接必须是 https:// 开头的。"
  - English: "The avatar link must start with https://."
- **Global availability** — the image service is region-agnostic; do not advise the user to switch region. Do not mention "CDN" to the user.
