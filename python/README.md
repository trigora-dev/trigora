Trigora Python authoring and client package.

```python
from trigora import effect, wait_for_event

async def run():
    result = await effect("generate", lambda: 42)
    approval = await wait_for_event("approved")
    return {"result": result, "approval": approval}
```

The `py.subset.v1` compiler accepts `async def run()` or `async def run(input)`. One plain parameter, or none. Not a default, `*args`, a keyword-only parameter, or a pattern.

This package is MIT and does not embed the TCC engine. Compile and recover through the `trigora` CLI. Directly calling the program function throws and tells you to use `trigora dev`.

```python
from trigora import start

run = start("approval")
run.send("approved", "ok")
print(run.result())
```

Requires `trigora dev` and `TRIGORA_RUNTIME_URL` (default `http://127.0.0.1:3477`).
