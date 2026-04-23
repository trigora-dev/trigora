# Flow Management

Trigora alpha includes a small set of hosted flow management commands.

These commands use the same deploy token as hosted deploys.

## List Flows

List deployed flows available to your token:

```bash
npx trigora flows
```

The list view includes:

- flow name
- hosted flow ID
- trigger type
- status
- endpoint for webhook flows
- schedule for cron flows
- queue name for queue flows

## Inspect A Flow

View a single hosted flow in more detail:

```bash
npx trigora flows inspect <flowId>
```

Depending on the flow type, this can include:

- name
- ID
- trigger
- status
- creation time
- endpoint and route
- schedule
- queue name

## Disable A Flow

Disable a hosted flow without deleting it:

```bash
npx trigora flows disable <flowId>
```

This is useful when you want to stop traffic or pause a flow temporarily.

## Enable A Flow

Re-enable a previously disabled flow:

```bash
npx trigora flows enable <flowId>
```

This restores the flow to an active hosted state when supported by the deployment.

## Authentication

All hosted flow management commands require:

```bash
TRIGORA_DEPLOY_TOKEN=your-deploy-token
```

## Current Scope

The current alpha flow management surface includes:

- list
- inspect
- disable
- enable

Delete is not available yet from the CLI.
