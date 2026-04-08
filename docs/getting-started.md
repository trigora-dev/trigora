# Getting Started

## 1. Install

```bash
npm install trigora @trigora/sdk
```

## 2. Initialize a project

```bash
npx trigora init
```

This creates:

- flows/hello.ts
- payload.json

## 3. Run your flow

```bash
npx trigora dev hello --payload payload.json
```

You’ll see logs in real time and the flow will re-run on changes.

---

## Development workflow

- edit your flow
- save
- Trigora re-runs automatically

---

## Next steps

- modify payload.json
- create new flows
- explore ctx.log and ctx.env
