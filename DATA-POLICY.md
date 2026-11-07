# Data policy

CFDCraft collects nothing about you. It has no accounts, no telemetry, no crash reports, no analytics and no update check. Your studies, case folders and results stay where you keep them.

## What the program does with the network

- **Nothing by default.** `compile`, `export`, `check-case` and the other offline commands read and write local files only.
- **Documents stay on disk.** The document commands `save`, `open`, `link`, `check-links`, `pack` and `unpack`, and the matching document tools of `mcp`, read and write local files only and contact no network. A pack is a file you move yourself; unpacking reads only the pack you give it.
- **Remote runs go only to the host you name.** `run`, `validate` and the run tool of `mcp` start a solver in a container on a host you configure in your own `hosts.toml`, over SSH with a key you choose. The study and case files are sent to that host, and the results are copied back. Nothing is sent to Milleleaf or any other service.
- **Your credentials stay yours.** The program reads the key path you give it and passes it to `ssh`. It does not store, log or upload keys, and it does not put the host address in the case it writes.

## Questions and changes

Questions about privacy: hello@milleleaf.com.

Any change to this policy is made in this file, so its history is public.
