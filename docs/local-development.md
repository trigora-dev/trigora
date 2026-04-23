# Local Development

Trigora is designed to make local iteration fast and predictable.

The usual workflow is:

1. define a flow in `flows/`
2. run it once with `trigger`
3. switch to `dev` while you iterate

## Run A Flow Once

Use `trigger` when you want a single local execution:

```bash
npx trigora trigger hello --payload payload.json
```

This is useful for:

- quick validation
- trying different payloads
- reproducing behavior without watch mode

## Run In Watch Mode

Use `dev` when you want a tighter edit-and-run loop:

```bash
npx trigora dev hello --payload payload.json
```

`dev` watches:

- the selected flow file
- the payload file, when one is provided

When either file changes, Trigora re-runs the flow automatically.

## Flow Resolution

Commands that take a `<flow>` argument accept either a flow name or a path.

Examples:

```bash
npx trigora trigger hello
npx trigora trigger ./flows/hello.ts
npx trigora dev hello --payload payload.json
```

The CLI resolves flows in this order:

1. the exact path you passed
2. `flows/<name>.ts`
3. `flows/<name>.js`
4. `<name>.ts`
5. `<name>.js`

## Payload Files

Payload files should be valid JSON.

Example:

```json
{
  "message": "Hello from Trigora"
}
```

If you do not pass `--payload`, the payload defaults to an empty object.

## Environment Files

The CLI automatically reads:

- `.env`
- `.env.local`

Shell environment variables take precedence over values loaded from these files.

This makes it easy to keep local configuration and deploy tokens out of source control.

## What Local Development Is Best For

Local development is especially useful for:

- iterating on flow logic
- testing payload handling
- refining logs
- validating trigger configuration

When the flow is ready for a hosted webhook environment, move on to [Hosted Deploys](/Users/omarabd/Documents/GitHub/trigora/docs/hosted-deploys.md).
