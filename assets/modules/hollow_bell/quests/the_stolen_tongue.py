# The Hollow Bell — "The Stolen Tongue".
#
# Tobin cannot pour a clapper. Two hundred years of getting hit is *in* that
# lump of iron and you cannot pour that; you can only fetch it back. It is
# currently a hat. Kill Cinderjack.

import mud_quest_api as q

title = "The Stolen Tongue"

subscribes_to = ["ObjectKilled"]

TARGET = "hollow_bell/cinderjack"

state = {"slain": False}


def on_start(state):
    state["slain"] = False
    q.set_var("hollow_bell_tongue_started", True)
    q.set_var("hollow_bell_tongue_ready", False)
    q.log("hollow_bell/the_stolen_tongue: started")


def on_event(ev, state):
    if ev["kind"] != "ObjectKilled":
        return
    if ev["type_id"] != TARGET:
        return
    state["slain"] = True
    # The clapper is a guaranteed corpse drop, so the player still has to go
    # and pick it up — the Yarn side gates the hand-in on has_item as well.
    q.set_var("hollow_bell_tongue_ready", True)
    q.log("hollow_bell/the_stolen_tongue: Cinderjack down")


def on_command(name, args, state):
    if name == "complete":
        q.set_var("hollow_bell_tongue_done", True)
        q.complete_quest("hollow_bell/the_stolen_tongue")
