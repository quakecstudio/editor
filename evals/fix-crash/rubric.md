# Fix a crash on spawn: reviewer rubric

A run that passes its scenario is then read by a reviewer against these:

- The cause is found: a call through th_pain before it is set, in PutClientInServer.
- The fix removes the call; it does not hide the error by checking for a null function everywhere.
- No unrelated files changed.
