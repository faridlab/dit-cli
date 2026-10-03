---
name: API Contract
summary: What each endpoint takes and returns — pointing at the OpenAPI file and the Morse scenarios that prove it.
folder: docs/technical
---
# {{title}}

*Who reads this: the teams building and calling the API. Started {{date}} by {{author}}.*

## Overview

*What this API is for, who calls it, and the base address in each environment.*

## Source of truth

*The OpenAPI file in this repository is the contract; this page explains it. Register it as a Morse spec so its operations can be proven against a running service.*

## Authentication

*How a caller proves who it is, and which scopes or roles each part needs.*

## Endpoints

| Method | Path | Purpose | Proven by |
|---|---|---|---|
| *POST* | */orders* | *Place an order* | *Morse scenario name* |

## Errors

*The error shape, and what each status code means for a caller.*

## Versioning and change

*How breaking changes are announced and how long old versions live.*

## Related

*The data model behind it, the TSD that builds it, the issues that change it.*
