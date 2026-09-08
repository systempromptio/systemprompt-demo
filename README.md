<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://systemprompt.io/files/images/logo.svg">
  <source media="(prefers-color-scheme: light)" srcset="https://systemprompt.io/files/images/logo-dark.svg">
  <img src="https://systemprompt.io/files/images/logo-dark.svg" alt="systemprompt.io" width="380">
</picture>

# See AI governance enforce a decision.

**Connect a client. Run a tool. Inspect the decision.**

This repository powers the SystemPrompt demo: a working application built on the same extensible runtime you can deploy inside your own organization.

[Open the demo](https://demo.systemprompt.io) · [Run locally](#run-locally) · [Evaluation template](https://github.com/systempromptio/systemprompt-template) · [Core](https://github.com/systempromptio/systemprompt-core)

</div>

## Try the governed workflow

Open [demo.systemprompt.io](https://demo.systemprompt.io) and use the account and setup flow available there. The **Systemprompt Bridge** connects supported Claude clients—Claude Code, Claude Desktop and Cowork—to the configured gateway and MCP services. Follow the setup page for the appropriate client connection.

The [Bridge documentation](bridge/README.md) covers installation behavior, authentication and building the desktop app for macOS or Windows. A client configured by hand must also supply the gateway's required authentication and session information; see [gateway routing](docs/gateway-routes.md).

Once connected, try:

- **“What is systemprompt?”** Read the platform documentation through its MCP tools.
- **“How does governance work here?”** Inspect the policy explanation and demonstrate a refused tool call.
- **Run `demonstrate_tool_rejection`.** Request the deliberately blocked egress tool and inspect the denial.
- **Run `analyse_governance_stats`.** Review the recorded tool decisions and usage.

The useful proof is the chain: an authenticated request, a policy decision, an allowed or blocked action, and an audit record you can inspect.

## What the demo demonstrates

| Behavior | What to inspect |
|---|---|
| Model access | Inference sent through the gateway is associated with the caller and checked before dispatch. |
| Tool policy | Governed MCP calls pass scope, credential-pattern, blocklist and rate-limit checks. |
| Attribution | Request and tool records connect identity, decisions and usage in PostgreSQL. |
| Custom business logic | A credit extension adds a balance check through Core's gateway guard interface. |

Routing inference through a gateway governs those requests. Local client tools require their own integration. Credentials supplied to tool subprocesses do not need to enter model context; tool behavior and output still belong in the deployment's security review.

## An application built on Core

Core supplies identity, gateway, MCP, policy and audit capabilities. This repo adds onboarding, credit accounting, email and the demo experience through compiled extensions and configuration. The branded Bridge reuses Core's desktop implementation.

Inspect the [credit guard](extensions/credits/src/guard.rs) to see a domain rule attached to inference dispatch. It checks the user's balance and can return a structured denial. Its cache and error behavior are visible in the implementation; it is an example of extensibility, not a guarantee of a hard spending cap.

That is the composition model: **Core primitives + your capabilities → your deployment.** For a general starting point for your organization, use [systemprompt-template](https://github.com/systempromptio/systemprompt-template).

## Run locally

Install Docker, [just](https://just.systems/) and Rust through rustup. Use the pinned [Rust toolchain](rust-toolchain.toml); this workspace declares Rust 1.94+. Setup provisions PostgreSQL 18 and requests your AI provider key.

```bash
git clone https://github.com/systempromptio/systemprompt-demo
cd systemprompt-demo
just setup-local
just start
```

Open **http://localhost:8080**. Setup builds the binary, starts PostgreSQL, migrates the database and publishes the pages. Local inference is billed to your configured provider account. Demo credits account for usage inside the application; they do not fund your provider account.

`setup-local` alone does not grant a user credit. The onboarding approval path supplies the signup grant; gateway requests need an available balance. Explore the [scripted demos](demo/README.md) for enforcement and analytics, and [installation documentation](docs/README.md) for deployment options. Building the Bridge also requires a sibling Core checkout, as described in its documentation.

## License

This repository is [MIT licensed](LICENSE). The underlying [Core library](https://github.com/systempromptio/systemprompt-core/blob/main/LICENSE) is BSL-1.1, available for evaluation, testing and non-production use under its license terms. Production use requires a commercial license; each Core version converts to Apache-2.0 four years after publication.

[Try the demo](https://demo.systemprompt.io) · [Build your own deployment](https://github.com/systempromptio/systemprompt-template) · [Discuss production licensing](mailto:ed@systemprompt.io)
