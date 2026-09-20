from trigora import effect, wait_for_event

async def run():
    result = await effect("generate", lambda: 42)
    review = await wait_for_event("approved")
    return {"result": result, "review": review}
