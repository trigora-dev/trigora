# trigora

CLI for running and developing Trigora flows.

---

## Install

```bash
npm install trigora
```

---

## Commands

### init

```bash
trigora init
```

Creates starter files for your project.

---

### dev

```bash
trigora dev <flow> --payload <file>
```

Runs a flow in watch mode.

- re-runs on file changes
- shows structured logs

---

### trigger

```bash
trigora trigger <flow> --payload <file>
```

Runs a flow once.

---

## Flow resolution

```bash
trigora dev hello
trigora dev ./flows/hello.ts
```

---

## Example output

```
[hello] RUN starting
[hello] INFO Hello from Trigora
[hello] RUN succeeded (3ms)
```
