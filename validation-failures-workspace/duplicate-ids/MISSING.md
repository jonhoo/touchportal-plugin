# Missing Validation: Duplicate Action IDs

**Issue**: Multiple actions with identical IDs are allowed to compile successfully.

**Test**: This plugin compiles successfully despite having two actions with ID `duplicate_action`.

**Why this should be caught**: Duplicate IDs cause conflicts in TouchPortal's internal state management and unpredictable behavior.

**Expected Behavior**: Should fail with error `duplicate action ID 'duplicate_action' found`

**Actual Behavior**: Compiles successfully, no validation error.

## Test Configuration

This test plugin intentionally creates two actions with the same ID to verify that the SDK should catch this configuration error at build time.

When this validation is implemented in the SDK, this test will start failing compilation and should be moved to a proper validation test with an `expected-error.txt` file.

## Note on Code Generation

While duplicate IDs don't cause build script validation errors, they may cause Rust compilation errors due to duplicate generated function names. However, this results in confusing compiler errors rather than clear validation messages, so proper build-time validation is still needed.