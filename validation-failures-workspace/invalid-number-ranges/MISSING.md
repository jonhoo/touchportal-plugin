# Missing Validation: Number Data Initial Values Outside Range

**Issue**: Number data fields with initial values outside their min/max bounds are allowed to compile successfully.

**Test**: This plugin compiles successfully despite having initial value `150.0` with min `0.0` and max `100.0`.

**Why this should be caught**: Initial values outside valid ranges cause runtime errors and poor user experience. TouchPortal UI may show invalid default values.

**Expected Behavior**: Should fail with error `initial value 150 is outside the allowed range [0, 100]`

**Actual Behavior**: Compiles successfully, no validation error.

## Test Configuration

This test plugin intentionally creates a number data field with an initial value outside the specified min/max range to verify that the SDK should catch this configuration error at build time.

When this validation is implemented in the SDK, this test will start failing compilation and should be moved to a proper validation test with an `expected-error.txt` file.