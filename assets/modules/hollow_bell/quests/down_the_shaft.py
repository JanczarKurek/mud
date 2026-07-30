# The Hollow Bell — "Down the Shaft".
#
# Marten will not let anyone near the cage-lift until the haulage-way is
# passable. Kill 8 sump crawlers in the Winding Works.
#
# Yarn variables are a global namespace, so every one this module touches is
# prefixed `hollow_bell_`.

import mud_quest_api as q

subscribes_to = ["ObjectKilled"]

TARGET = "hollow_bell/sump_crawler"
NEEDED = 8

state = {"crawlers": 0}


def on_start(state):
    state["crawlers"] = 0
    q.set_var("hollow_bell_shaft_started", True)
    q.set_var("hollow_bell_shaft_ready", False)
    q.log("hollow_bell/down_the_shaft: started")


def on_event(ev, state):
    if ev["kind"] != "ObjectKilled":
        return
    if ev["type_id"] != TARGET:
        return
    state["crawlers"] = state["crawlers"] + 1
    q.log("hollow_bell/down_the_shaft: crawlers={}/{}".format(state["crawlers"], NEEDED))
    if state["crawlers"] >= NEEDED:
        q.set_var("hollow_bell_shaft_ready", True)


def on_command(name, args, state):
    if name == "complete":
        q.set_var("hollow_bell_shaft_done", True)
        q.complete_quest("hollow_bell/down_the_shaft")
