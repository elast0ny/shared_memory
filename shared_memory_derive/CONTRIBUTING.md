# Contributing

## Testing

This uses the [`trybuild`] crate to test whether certain scenarios compile or
fail to compile. Using this crate is particularly convenient because it helps
test that the compiler output produced on failure is actually reasonably
readable and correct. The compiler error messages are stored in a file and
compared against the output produced during the test.

See the documentation for that crate for more information.

The `tests/ui` directory is to test the error messages produced by the macro.
All files in this directory should fail to compile. There should be a `.stderr`
file associated with each file that shows all of the expected error messages.

The `tests/run-pass` directory contains files that should definitely compile.
