# Formatter updates

Whenever you change the Ube formatter, also update the user's active installed
VS Code extension with the changed formatter files. Locate the active installation
instead of assuming a fixed version, verify it matches the workspace formatter,
and run a formatting smoke check against the installed copy. The user has
explicitly requested this as part of every formatter change.
