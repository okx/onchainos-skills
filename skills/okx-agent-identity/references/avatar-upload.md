# Avatar Upload — Runtime Decision Matrix

`--picture` on `agent create` / `agent update` takes a URL. To get a URL, you either (a) have one already, or (b) run `onchainos agent upload <file>` to mint a CDN URL from a local image.

The right path depends on the runtime. Do not force the user down a path their environment cannot support.

| Runtime | User provides image | AI-generated image | Skip avatar |
|---|---|---|---|
| **Claude Code (desktop / IDE)** | ✓ save attachment to a temp path → `agent upload <path>` → take returned URL | ✓ call the image-gen tool → save to temp path → `agent upload <path>` → URL | ✓ omit `--picture`; backend assigns default |
| **Plain terminal / CLI chat** | ✗ no file inline — do NOT ask the user to locate a path on disk | ✓ describe the prompt to the image-gen tool (user cannot preview but the URL works) | ✓ omit `--picture` |
| **User writes the command themselves** | ✓ they pass `--picture <url>` directly | N/A | ✓ they omit `--picture` |

---

## Policy

1. **Detect the runtime first.** If the session has an attachment facility (Claude Code attachments, editor drag-drop), allow "user-provided image". If it is a terminal-only chat, do not ask the user for a path.
2. **Default to "skip".** If the user doesn't bring up avatar, do not prompt for one; create/update succeeds with a backend-assigned default image.
3. **Offer exactly two options in terminal mode**: "描述一个让我生成" or "跳过用默认图". Do not add a third "上传本地图片" option — that path will fail.
4. **AI-generation requires explicit prompt.** Do not invent image content. Ask the user: "你希望头像长什么样？几个关键词就够。"
5. **Upload response is a URL.** Store it and pass as `--picture`. Never try to re-upload the same URL; the CDN URL is stable.

---

## Claude Code flow (attachment-supported)

```
User: "帮我注册 provider，名字 Alice，顺便用这张图做头像" [attaches file]
Skill:
  1. Save attachment → /tmp/<random>.png
  2. onchainos agent upload /tmp/<random>.png
     ← { url: "https://cdn.example.com/u/abc.png" }
  3. Run agent create with --picture "https://cdn.example.com/u/abc.png"
  4. Clean up the temp file
```

## Claude Code flow (AI-generated)

```
User: "用一只戴眼镜的青蛙当头像"
Skill:
  1. Call image-gen tool with prompt → /tmp/<random>.png
  2. Show the generated image to the user, confirm
  3. onchainos agent upload /tmp/<random>.png → URL
  4. Proceed with agent create / update
```

## Terminal flow (no attachments)

```
User: "上传个头像"
Skill: "当前环境没法直接上传本地图片。要让我用关键词帮你生成一张，还是先跳过用系统默认头像？"
  - "生成" → ask keywords → image-gen → upload → URL
  - "跳过" → proceed without --picture
```

## User-provided URL

If the user already has a URL (e.g., "用这个 twitter 头像 https://..."), trust it and pass directly as `--picture`. Do not re-download and re-upload.

---

## Validation

- **MIME type** — backend accepts PNG / JPEG / WebP; other types return `unsupported media type`. On that error, retry once after converting, or tell the user.
- **File size** — no explicit limit in the CLI; if backend rejects with a size error, ask the user to downscale (≤ 2 MB is a safe default).
- **URL shape** — must be HTTPS. Anything else: "头像链接必须是 https:// 开头。"
- **CDN availability** — the CDN is considered globally available; do not advise the user to switch region.
