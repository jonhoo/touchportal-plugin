# Validation Errors NOT Caught at Build Time

This document tracks plugin configuration mistakes that are NOT caught by the SDK's build-time validation but probably should be.

## Entry Format Template

Each validation issue should be documented with the following structure:

**Issue**: Brief description of the configuration problem
**Test**: Name of test plugin and what it attempted
**Why this should be caught**: Reasoning for why it should fail validation
**Expected Behavior**: What error message should appear
**Actual Behavior**: What actually happens (compiles successfully, wrong error, etc.)

## Validation Issues With Test Crates

Some validation issues already have dedicated test crates in this workspace that currently compile when they should not. See the individual `MISSING.md` files in each test crate directory for detailed information.

## Currently Uncaught Validation Issues (No test crates yet)

*No uncaught validation issues without test crates at this time.*

## Potential Future Test Crates

Additional validation scenarios that could benefit from dedicated test crates:

- **Actions exceeding maximum line limits**: TouchPortal may have UI constraints
- **Missing connector data**: Connectors without required data fields

Find more of these, for example, by exploring the TouchPortal API and thinking of other configurations that should ideally fail at compile-time.
Don't consider whether the code generation _currently_ catches those mistakes. Write the tests first, and if the errors are _not_ caught, add them here or above.
