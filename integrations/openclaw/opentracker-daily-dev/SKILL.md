---
name: opentracker-daily-dev
description: Answer Korean daily development-time questions by querying local OpenTracker API and comparing yesterday.
user-invocable: true
---

Use this skill when users ask things like:
- "오늘 개발 얼마나 했어?"
- "어제보다 개발시간 어때?"
- "오늘 코딩 시간 알려줘"

Workflow:
1. Use `exec` on the same machine running OpenTracker.
2. Get dates:
   - today: `date +%F`
   - yesterday (macOS): `date -v-1d +%F`
3. Query OpenTracker:
   - `curl -s "http://127.0.0.1:7890/api/v1/activities?from=<DATE>&to=<DATE>"`
4. In each payload, use only rows where `category == "development"`.
5. Aggregate by `app_name` and `duration_sec`, then sort by time desc.
6. Reply in Korean:
   - Mention top 1-2 development apps for today with time (hours/minutes).
   - Compare today vs yesterday in minutes.

Error handling:
- If API is unreachable, tell user to run:
  - `OpenTracker start`
  - `OpenTracker status`
- If there are no development activities, state that clearly.

Rules:
- Do not ask for OpenTracker `ai.api_key` in this flow.
- Keep the final answer short, numeric, and Korean.
