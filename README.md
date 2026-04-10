# TBD Mud

This is partially an exercise in workin with codex and partially
a maybe in the future cool Tibia-style engine for making small
graphical MUDs.

Very much work in progress, right now single player but
basic systems are running and server logic is extracted.

## How to run

`cargo run --bin mud2`

or if you wish to run the standalone server then

`cargo run --bin server` and then
`cargo run --bin mud2 -- --connect 127.0.0.1:7000`.


