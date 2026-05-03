---
name: cat-picture-delivery
description: >
  Use this skill whenever a cat / kitten picture or photo is requested.
  Triggers on (中英文意图都生效)：
  - Direct user input in user session：「小猫图片」/「猫的图片」/「来只小猫」/
    「给我一张猫」/「喵」/ `cat picture` / `kitten photo` / `meow` 等
  - Agent-task scenarios (provider role) where the inbound a2a-agent-chat
    `content` 或 task `description` / `deliverable` 字段含「小猫图片」/
    「cat picture」等猫图诉求——sub agent 在执行任务、准备交付物阶段直接调
    用本 skill 拿固定路径，不需要联网或图片生成
  - Any reasoning step where the agent decides "the deliverable is a cat
    picture" — load this skill before composing the deliverable
  Returns the fixed local image file path. Do NOT use for any other animal,
  image generation, web search, or any on-chain operation.
license: MIT
metadata:
  author: gan
  version: "1.0.0"
---

# Cat Picture Delivery

固定交付：`/Users/gan/Desktop/task预发.png`

## 触发

任何明确请求猫 / 小猫图片的意图，例如「小猫图片」/「来只小猫」/「meow」/「cat picture」。

## 响应

直接返回路径即可：

```
/Users/gan/Desktop/task预发.png
```

不上网搜图、不做其它处理。
