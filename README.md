# Trigora

**Run code when things happen.**

Write flows locally. Deploy them globally. No infrastructure.

Trigora is a local-first, event-driven automation platform for developers.  
Define flows in code, test them locally with real data, and deploy the same flows to run globally.

---

## ⚡ Why Trigora?

Automation today is fragmented:
- scripts
- cron jobs
- webhook handlers
- background workers

Trigora unifies all of it into one model.

- ✍️ Write flows in TypeScript  
- ⚡ Run and debug locally  
- 🧪 Test with real payloads  
- 🌍 Deploy globally with zero infrastructure  

---

## ✨ Quick example

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'manual' },
  async run(event, ctx) {
    await ctx.log.info('Hello from Trigora', {
      payload: event.payload,
    });
  },
});
```

---

## 🚀 Get started in 30 seconds

```bash
npm install trigora @trigora/sdk
npx trigora init
npx trigora dev hello --payload payload.json
```

- edit your flow  
- save  
- it re-runs instantly  

No deploy step. No setup.

---

## 🧠 Local-first development

```bash
trigora dev hello --payload payload.json
```

- run flows locally  
- test with real payloads  
- instant feedback loop  

This is your primary development workflow.

---

## 🌍 Deploy globally (coming soon)

```bash
trigora deploy
```

Take the exact same flow you tested locally and run it globally.

- 🚫 no infrastructure setup  
- 🌎 globally distributed execution  
- ⚙️ built-in triggers:
  - webhooks  
  - schedules / cron  
  - queues  
- 📦 production-ready runtime  

> **Write once. Run locally. Deploy globally.**

---

## 🖥️ Platform (coming soon)

Trigora is evolving into a full platform for event-driven systems.

### UI Dashboard

- view and manage flows  
- inspect executions and logs  
- monitor failures and retries  
- manage triggers and environments  

---

### Built-in triggers

- webhooks (instant endpoints per flow)  
- scheduled jobs / cron  
- queues and background processing  

---

### Observability

- structured logs  
- execution history  
- error tracking  
- replay events  

---

### Secrets & environments

- secure environment variables  
- per-flow configuration  
- multi-environment support  

---

### Edge execution

Powered by Cloudflare:

- low-latency execution  
- automatic scaling  
- no infrastructure management  

---

## 📦 Packages

- `trigora` — CLI (run + deploy flows)  
- `@trigora/sdk` — flow definition API  
- `@trigora/contracts` — shared types  

---

## 🏗️ Repository

This repository contains the open-source Trigora developer toolkit:

- CLI  
- SDK  
- Contracts  
- Documentation  

---

## 📊 Status

Early development.

- ✅ Local development is stable  
- 🚧 Global deployment in progress  
- 🚧 Platform (UI + API) in development  

---

## ⭐️ Support

If you find Trigora interesting, consider giving it a star ⭐️

---

## 📄 License

MIT
