# TBD Mud

This is partially an exercise in workin with codex and in the future maybe a cool Tibia-style engine for making small
graphical MUDs.

## Disclaimer
This is basically almost entirely vibe-coded with some rather mild
architecture-level steering. That means, the ideas for the systems
and the development directions is for now human-based.

All the assets for now are AI-generated placeholders, mostly
not the bad type of ai generated art but ya know, codex writing
pixelart with imagemagick by hand.

Given the fact that I have no idea how most of this code works,
I give zero warranty whatsoever about whether running this code
is a security risk.

## What's here
Very much work in progress. For now things that work:
- Movement
- Equipment system
- Basic battle and magic systems
- Persistant world
- Multiple world instances (spaces)
- Multiplayer

## How to run

`cargo run --bin mud2`

or if you wish to run the standalone server then

`cargo run --bin server` and then
`cargo run --bin mud2 -- --connect 127.0.0.1:7000`.


