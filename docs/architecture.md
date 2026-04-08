# Architecture

Trigora is structured as a layered system.

---

## Packages

- **trigora** — CLI runtime
- **@trigora/sdk** — developer API
- **@trigora/contracts** — shared types

---

## Flow Execution

1. CLI loads the flow module
2. Payload is read and parsed
3. Event object is constructed
4. Flow is executed
5. Logs are emitted

---

## Design Principles

### Local-first

All flows can run locally without external dependencies.

### Explicit

No hidden magic. Everything is defined in code.

### Portable

Flows are plain TypeScript modules.

---

## Runtime Model

```txt
CLI → Load Flow → Create Event → Execute → Log Output
```

---

## Future Direction

- distributed execution
- remote triggers
- persistent state
- observability tools
