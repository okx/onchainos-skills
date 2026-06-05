# Avatar Upload — Runtime Decision Matrix

`--picture` on `agent create` / `agent update` takes a URL. To get a URL, you either (a) already have one, or (b) run `onchainos agent upload --file <path>` to get one from a local image. **The "CDN" / "upload pipeline" are implementation details — never expose these words to the user.** To the user it's simply "send me an image, I'll handle the upload".

The right path depends on the runtime. Do not force the user down a path their environment cannot support.

| Runtime | User provides image | AI-generated image | Skip avatar |
|---|---|---|---|
| **Claude Code (desktop / IDE)** | ✓ save attachment to a temp path → `agent upload --file <path>` → take returned URL | ✓ call the image-gen tool → save to temp path → `agent upload --file <path>` → URL | ✓ omit `--picture`; backend assigns default |
| **Plain terminal / CLI chat** | ✗ no file inline — do NOT ask the user to locate a path on disk | ✓ describe the prompt to the image-gen tool (user cannot preview but the URL works) | ✓ omit `--picture` |

---

## Policy

1. **Image links are not accepted.** If the user provides a URL (e.g. "use this avatar https://..."), reject it immediately: "Avatar links are not supported — please send an image file directly, or say 'generate' to create one." Do NOT pass any user-supplied URL to `--picture`.
2. **Detect the runtime first.** If the session has an attachment facility (Claude Code attachments, editor drag-drop), allow "user-provided image". If it is a terminal-only chat, do not ask the user for a path.
2. **Skip is the default, but it is actively prompted at the Step-1 identity card's close — not buried in a row.** Do **not** ask the avatar as its own collection turn. In the provider `agent create` flow the **identity card (Step 1)** shows `Profile photo: default (not set)` and the card's **closing CTA actively invites setting one** (the 📷 line — "send an image or say 'generate'"; see `playbooks/provider.md §Confirmation cards — two steps`). A faint "say add avatar" row hint is too passive — real runs showed users never act on it, leaving every agent on the default image; the active close-CTA is required. Run the avatar flow (below) when the user opts in there (image / "generate"), or brought up an avatar earlier. Replying "next" = skip → backend-assigned default image. For `agent update`: never re-prompt unless the user raises it.
3. **When prompting, match the user's language and use the numbered-options pattern:**
   - **Claude Code (attachments supported) — 3 options:** want an avatar? → 1. send image (upload it, recommend 1:1 PNG/JPEG/WebP, no rounded corners or borders — see §Policy 7) / 2. generate from keywords / 3. skip (default avatar). Reply 1/2/3.
   - **Terminal (no attachments) — 2 options:** open with "can't receive attachments here" — then: 1. generate from keywords (1:1 recommended) / 2. skip (default avatar). Reply 1/2.
4. **AI-generation requires explicit prompt.** Do not invent image content. Ask the user in their language: "Describe the avatar — a few keywords is enough."
5. **Upload result is a URL — show it to the user.** Pass it to `--picture`, and render the URL verbatim in the Picture row of the confirmation card and the detail card (see `core/display-formats.md §Picture row rule`). Do **not** hide the URL behind "uploaded" or any placeholder. Do not re-upload an already-uploaded image.
6. **Aspect ratio guidance.** When the user sends a non-1:1 image, accept it and upload anyway — do not reject or demand re-crop. But when *proactively* recommending dimensions, say "1:1 square" rather than a specific pixel size like 512×512 (the backend does not enforce 512, and pixel specifics date quickly).
7. **Display-quality tip (advisory, never blocking).** When prompting the user to *send/upload* an image — at the moment you offer the "send image" option, and again right before you accept an attachment — surface a one-line tip **in the user's language**: avoid **rounded corners** and avoid a **border/frame** — a plain full-bleed square renders best on the marketplace. This is purely a recommendation: if the user sends a rounded/bordered image anyway, accept it and upload — do NOT reject, do NOT auto-crop or strip the border (altering the user's image is forbidden, see §Validation). Only mention it at upload time; do not raise it for AI-generated avatars unless the user is choosing reference imagery.
   - Example phrasing (localize, do not hard-code any one language): "For the best display, avoid rounded corners and borders — a plain square image works best."

---

## Claude Code flow (attachment-supported)

```
User: "Register a provider named Alice, use this image as the avatar" [attaches file]
Skill:
  1. Save attachment → /tmp/<random>.png
  2. Check file size — if > 1 MB: STOP, prompt the user (see Validation §File size), do NOT proceed to step 3
  3. onchainos agent upload --file /tmp/<random>.png
     ← { url: "<url>" }
  4. Run agent create with --picture "<url>"
  5. Clean up the temp file
```

After the upload succeeds, move to the next step of the flow silently — or with a one-line ack that includes the URL so the user can see what was set:
- "Got it, avatar set: `<url>`"

The URL **must** appear verbatim in the Picture row of the confirmation card (`core/display-formats.md §Picture row rule`), and in the detail card after success. Never replace it with "uploaded" / "CDN" or any placeholder phrase. Never mention the word "CDN" to the user.

## Claude Code flow (AI-generated)

```
User: "Use a frog wearing glasses as the avatar"
Skill:
  1. Call image-gen tool with prompt → /tmp/<random>.png
  2. Show the generated image to the user, confirm ("Does this work?")
  3. onchainos agent upload --file /tmp/<random>.png → URL
  4. Proceed with agent create / update
```

## Terminal flow (no attachments)

```
User: "Upload an avatar"
Skill: (render the 2-option numbered prompt — see Policy §3, Terminal variant)
  - User replies "1" / "generate" → ask keywords → image-gen → upload → URL (silently)
  - User replies "2" / "skip" → proceed without --picture
  - Anything else → re-render the numbered prompt once; never silently default.
```

## User-provided URL

⛔ **Not supported.** If the user provides a URL, reject it with:
- "Avatar links are not supported — please send an image file directly, or say 'generate' to create one."

Do NOT pass any user-supplied URL to `--picture`. Do NOT attempt to download and re-upload the URL.

---

## Validation

- **MIME type** — backend is known to accept PNG / JPEG / WebP; other types are likely rejected with a backend-originated error (exact wording is not a CLI `bail!` and may drift — do NOT hard-code). On rejection, ask the user to convert to PNG / JPEG / WebP and retry.
- **File size** — hard limit is **1 MB**. Check the file size **before** calling `onchainos agent upload`. If the file exceeds 1 MB:
  - ⛔ **Do NOT call `onchainos agent upload` or any backend API.**
  - ⛔ **Do NOT proactively compress, resize, or modify the file.** The user owns the image; altering it without explicit instruction is forbidden.
  - Prompt the user in their language to supply a smaller image and stop the upload flow:
    - "The image exceeds 1 MB (~X MB) and can't be uploaded. Please compress it or send a smaller image (under 1 MB)."
  - Replace `X` with the actual file size rounded to one decimal place (e.g. `1.4 MB`). If the exact size is unavailable, omit the parenthetical size note.
- **Global availability** — the image service is region-agnostic; do not advise the user to switch region. Do not mention "CDN" to the user.
