# Hunter quest — kill 3 rats.
#
# Uses the event-subscription path (the "no firehose" case): this module is
# only invoked when a matching ObjectKilled event fires while the player has
# active state for this quest. Simple fetch-style quests ("bring N of X")
# don't need event subscription at all and can live entirely in .yarn files.

import mud_quest_api as q

subscribes_to = ["ObjectKilled"]

state = {"rats": 0}


def on_start(state):
    state["rats"] = 0
    q.set_var("hunter_started", True)
    q.set_var("hunter_ready", False)
    q.log("hunter quest: started")


def on_event(ev, state):
    if ev["kind"] != "ObjectKilled":
        return
    if ev["type_id"] != "rat":
        return
    state["rats"] = state["rats"] + 1
    q.log("hunter quest: rats={}".format(state["rats"]))
    if state["rats"] >= 3:
        q.set_var("hunter_ready", True)


def on_command(name, args, state):
    if name == "complete":
        q.log("hunter quest: completing")
        q.complete_quest("hunter")
