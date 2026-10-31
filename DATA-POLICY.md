# Data policy

CFDCraft collects nothing about you. It has no accounts, no telemetry, no crash reports, no analytics and no update check. Your studies, case folders and results stay where you keep them.

## What the program does with the network

- **Nothing by default.** `compile` and the other offline commands read and write local files only.
- **Remote runs go only to the host you name.** `run` and `validate` start a solver in a container on a host you configure in your own `hosts.toml`, over SSH with a key you choose. The study and case files are sent to that host, and the results are copied back. Nothing is sent to Milleleaf or any other service.
- **Your credentials stay yours.** The program reads the key path you give it and passes it to `ssh`. It does not store, log or upload keys, and it does not put the host address in the case it writes.

## Questions and changes

Questions about privacy: hello@milleleaf.com.

Any change to this policy is made in this file, so its history is public.
