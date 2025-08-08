# Missing Validation: Empty Required Fields

**Issue**: Actions with empty names are allowed to compile successfully.

**Test**: This plugin compiles successfully despite having an action with empty string name.

**Why this should be caught**: Empty names create poor user experience and unusable UI elements in TouchPortal.

**Expected Behavior**: Should fail with error `action name cannot be empty`

**Actual Behavior**: Compiles successfully, no validation error.

## Test Configuration

This test plugin intentionally creates an action with an empty name field to verify that the SDK should catch this configuration error at build time.

When this validation is implemented in the SDK, this test will start failing compilation and should be moved to a proper validation test with an `expected-error.txt` file.

## Scope

This test focuses on empty action names, but similar validation should apply to other required fields like:
- Action descriptions
- Category names
- State descriptions
- Event names

Additional test cases could be created for other required field types as needed.