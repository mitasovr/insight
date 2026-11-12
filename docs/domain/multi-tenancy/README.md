# Multi-tenancy

In plain words: what it takes for one Insight install to serve many tenants, who manages what, and how people sign in. Tracking issue: [#4299](https://github.com/constructorfabric/insight/issues/4299).

## TL;DR

- Today an install serves exactly one tenant, and its id is a chart value.
- The goal is many tenants in one install, with two kinds of admin:
  - an **instance admin** manages tenants;
  - a **tenant admin** configures their own tenant: sign-in, users, connectors.
- Every tenant shares one domain. In Keycloak, a tenant is an Organization. A tenant may bring its own identity provider, or have its users live in Keycloak.
- Data isolation ([#3661](https://github.com/constructorfabric/insight/issues/3661)) must land before a second tenant exists in an install.

## Where things stand

- **The tenant is a chart value.** It is set by `authenticator.oidc.defaultTenantId`, plus copies under `global.tenantDefaultId` and `ingestion.reconcile.tenantId`. The Keycloak config job stamps that one value into every login.
- **There is no tenant registry.** No table anywhere lists the tenants of an install.
- **There is no instance-level admin.** Every admin is a role assignment inside one tenant, and the first admin is seeded into the default tenant at install time.
- **Connectors are K8s Secrets.** Someone with cluster access applies them, and the reconcile job turns them into Airbyte and Argo objects, stamping them with the install-wide tenant. There is no API or UI for creating a connector.

## What needs to be done

### 1. Data isolation

Rows of different tenants never mix, and a service cannot read another tenant's rows:

- one tenant value end to end;
- one tenant column name from staging up;
- tenant-scoped dbt;
- ClickHouse row policies;
- tenant-aware analytics, insight-v3-core and MariaDB.

This is the [#3661](https://github.com/constructorfabric/insight/issues/3661) series; [#3875](https://github.com/constructorfabric/insight/pull/3875) explains it. It is a hard prerequisite for everything below.

### 2. Tenant registry

A tenant becomes a database record, not a chart value. Roughly:

- id (UUID) and name;
- status: active, suspended, deleting;
- sign-in setup: the Keycloak organization, verified e-mail domains, an identity provider if there is one;
- quotas.

Once the registry exists, the chart copies of the tenant go away. Related: [#725](https://github.com/constructorfabric/insight/issues/725).

### 3. Instance admin

A level above tenants:

- creates, suspends and deletes tenants;
- sets up a tenant's sign-in and appoints its first tenant admin;
- does **not** see tenant data. Any support access is an explicit, audited action.

The install creates the first instance admin, in place of today's bootstrap admin inside the default tenant.

### 4. Sign-in and routing

**One domain for every tenant, and a tenant is a Keycloak Organization.** There is one broker realm with one organization per tenant. The tenant in the token comes from the user's organization, not from a value hardcoded into each identity provider mapper.

- **A tenant with its own identity** (Entra, Okta and the like) links its identity provider to its organization, together with a verified e-mail domain. The user types an e-mail and Keycloak sends them to that provider (identity-first login).
- **A tenant without its own identity** keeps its users in Keycloak as members of the organization. The tenant admin invites them, and they sign in with a password or a passkey.
- **Both can live in one tenant:** employees through the company provider, invited contractors through local accounts.

The rules that keep this safe:

- **A domain belongs to exactly one tenant and must be verified.** Otherwise one tenant could claim another company's domain and capture its users.
- **Shared mail domains** (`gmail.com` and the like) are never bound to a tenant. Users on them get in by invitation only.
- **A person in several tenants** picks one after signing in. Switching tenants starts a new session; it never changes the claim inside the current one.

**Per-tenant subdomains** (`acme.insight.example`) are optional and can come later. They give Keycloak a direct hint and keep sessions apart, because the session cookie is bound to the host. They cost wildcard DNS, a wildcard TLS certificate and redirect URIs per host.

### 5. Tenant admin and connectors

- The tenant admin manages the users, roles and invitations of their tenant.
- An API and a UI let the tenant admin create connectors and their secrets. Secrets are never stored in plain text.
- Ingestion takes the tenant from each connector's record, never from an install-wide setting.
- Connector health and similar views show the caller's tenant only.
- Connectors run inside the cluster against URLs the tenant admin types in, so their egress has to be restricted (SSRF).

### 6. Tenant lifecycle

- **Suspend:** nobody can sign in, and syncs are paused.
- **Delete:** remove the tenant's data in every layer and store, its connectors and their Airbyte and Argo objects, and its organization and identity provider in Keycloak. This is one operation that can be verified and produces a report.

### 7. Operations

- Quotas and limits per tenant: connectors, sync frequency, query load ([#724](https://github.com/constructorfabric/insight/issues/724)).
- The tenant appears in logs, metrics and traces. A wrong tenant looks like "no data", not like an error, so it has to be visible.
- Audit of what instance admins and tenant admins do.

## Order

1. Data isolation (block 1) comes before a second tenant exists in an install.
2. Blocks 2–4 can run in parallel with it. They touch the authenticator, identity and Keycloak, not dbt or ClickHouse.
3. Connectors (block 5) come after the registry and sign-in.
4. Lifecycle and operations (blocks 6–7) come last.
