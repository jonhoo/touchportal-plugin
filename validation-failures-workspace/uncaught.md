# Validation Errors NOT Caught at Build Time

This document tracks plugin configuration mistakes that are NOT caught by the SDK's build-time validation but probably should be.

## 1. Empty Categories

**Issue**: Categories with no actions, events, states, or connectors are allowed to compile successfully.

**Test**: The `empty-category` plugin compiled without errors despite having a category with no content.

**Why this should be caught**: Empty categories serve no purpose and likely indicate a configuration error. TouchPortal UI would show empty categories to users, creating poor UX.

**Expected Behavior**: Should fail with a validation error indicating categories must contain at least one item.

**Actual Behavior**: Compiles successfully.

## Future Issues to Test

- **Actions with no lines**: Would create unusable buttons in TouchPortal UI
- **States with invalid number ranges**: Initial values outside min/max bounds cause runtime errors
- **File data with bad extensions**: Invalid formats like `.exe.` could cause file picker issues
- **Invalid plugin IDs**: Non-alphanumeric characters could break TouchPortal's plugin registry
- **Duplicate IDs**: Would cause conflicts in TouchPortal's internal state management
- **Missing required fields**: Empty names/descriptions create poor user experience