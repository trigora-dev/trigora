from trigora import start

run = start("approval")
print(f"started {run.id}")
print("waiting for approval; sending it now...")
run.send("approved", "ok")
print(run.result())
