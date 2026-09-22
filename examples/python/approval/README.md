# Approval (Python)

Same program as the TypeScript example: `effect` then `wait_for_event`. The Node `trigora` CLI compiles this through the Python frontend and runs it on the local TCC host.

```python
from trigora import effect, wait_for_event

async def run():
    result = await effect("generate", lambda: 42)
    review = await wait_for_event("approved")
    return {"result": result, "review": review}
```

The program id is the file stem (`approval`). Effect callbacks must be lambdas with no captures. This example uses `async def run()` with no parameter. The subset also allows `async def run(input)`.

## Run locally

Install the authoring package and the local engine wheel (Gate 1, macos/arm64/cp39 today):

```bash
pip install -e ../../python
pip install ../../dist-packages/tcc_engine-0.1.0rc1-cp39-cp39-macosx_11_0_arm64.whl
```

From the repo root:

```bash
pnpm install
pnpm build
```

Terminal 1, from this directory:

```bash
pnpm dev
```

Terminal 2:

```bash
python scripts/run.py
```

Kill and restart `pnpm dev` while the execution is waiting, then send `approved` to resume.

You can also drive the same runtime from the CLI:

```bash
trigora start approval
trigora executions inspect <id>
trigora send <id> approved --payload '"ok"'
```
