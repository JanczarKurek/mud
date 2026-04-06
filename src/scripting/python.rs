use std::sync::{Mutex, OnceLock};

use rustpython::InterpreterConfig;
use rustpython_vm::pymodule;
use rustpython_vm::scope::Scope;
use rustpython_vm::Interpreter;

use crate::scripting::resources::PythonConsoleState;

const BOOTSTRAP_SCRIPT: &str = r#"
import mud_api
world = mud_api

def _mud_print(*args, sep=" ", end=""):
    mud_api.log(sep.join(str(arg) for arg in args) + end)

print = _mud_print
"#;

static PYTHON_BRIDGE: OnceLock<Mutex<PythonBridgeState>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct WorldObjectSnapshot {
    pub object_id: u64,
    pub type_id: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug)]
pub struct PythonSnapshot {
    pub object_types: Vec<String>,
    pub objects: Vec<WorldObjectSnapshot>,
    pub player_position: (i32, i32),
}

#[derive(Clone, Debug)]
pub struct SpawnRequest {
    pub type_id: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Default)]
struct PythonBridgeState {
    snapshot: Option<PythonSnapshot>,
    output_lines: Vec<String>,
    spawn_requests: Vec<SpawnRequest>,
}

fn bridge_state() -> &'static Mutex<PythonBridgeState> {
    PYTHON_BRIDGE.get_or_init(|| Mutex::new(PythonBridgeState::default()))
}

pub struct PythonConsoleHost {
    interpreter: Interpreter,
    scope: Scope,
}

impl PythonConsoleHost {
    pub fn new() -> Self {
        let interpreter = InterpreterConfig::new()
            .init_stdlib()
            .add_native_module("mud_api".to_owned(), mud_api::make_module)
            .interpreter();

        let scope = interpreter.enter(|vm| {
            let scope = vm.new_scope_with_builtins();
            vm.run_code_string(scope.clone(), BOOTSTRAP_SCRIPT, "<mud-bootstrap>".into())
                .expect("Failed to initialize embedded Python console");
            scope
        });

        Self { interpreter, scope }
    }

    pub fn execute(
        &mut self,
        state: &mut PythonConsoleState,
        command: &str,
        snapshot: PythonSnapshot,
    ) -> Vec<SpawnRequest> {
        {
            let mut bridge = bridge_state().lock().expect("Python bridge mutex poisoned");
            bridge.snapshot = Some(snapshot);
            bridge.output_lines.clear();
            bridge.spawn_requests.clear();
        }

        let result = self
            .interpreter
            .enter(|vm| vm.run_code_string(self.scope.clone(), command, "<mud-console>".into()));

        match result {
            Ok(_) => {}
            Err(error) => {
                state.push_output(format!("Python error: {error:?}"));
            }
        }

        let mut bridge = bridge_state().lock().expect("Python bridge mutex poisoned");
        for line in bridge.output_lines.drain(..) {
            state.push_output(line);
        }
        bridge.snapshot = None;
        bridge.spawn_requests.drain(..).collect()
    }
}

#[pymodule]
mod mud_api {
    use rustpython_vm::convert::ToPyObject;
    use rustpython_vm::{PyObjectRef, VirtualMachine};

    use super::{bridge_state, SpawnRequest};

    #[pyfunction]
    fn log(message: String) {
        let mut bridge = bridge_state().lock().expect("Python bridge mutex poisoned");
        bridge.output_lines.push(message);
    }

    #[pyfunction]
    fn list_objects(vm: &VirtualMachine) -> PyObjectRef {
        let bridge = bridge_state().lock().expect("Python bridge mutex poisoned");
        let Some(snapshot) = &bridge.snapshot else {
            return vm.ctx.new_list(Vec::new()).into();
        };

        let objects = snapshot
            .objects
            .iter()
            .map(|object| {
                format!(
                    "id={} type={} pos=({}, {})",
                    object.object_id, object.type_id, object.x, object.y
                )
            })
            .map(|line| line.to_pyobject(vm))
            .collect();

        vm.ctx.new_list(objects).into()
    }

    #[pyfunction]
    fn list_object_types(vm: &VirtualMachine) -> PyObjectRef {
        let bridge = bridge_state().lock().expect("Python bridge mutex poisoned");
        let object_types = bridge
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.object_types.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.to_pyobject(vm))
            .collect();

        vm.ctx.new_list(object_types).into()
    }

    #[pyfunction]
    fn player_position(vm: &VirtualMachine) -> PyObjectRef {
        let bridge = bridge_state().lock().expect("Python bridge mutex poisoned");
        let (x, y) = bridge
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.player_position)
            .unwrap_or((0, 0));

        vm.ctx
            .new_tuple(vec![x.to_pyobject(vm), y.to_pyobject(vm)])
            .into()
    }

    #[pyfunction]
    fn spawn_object(type_id: String, x: i32, y: i32) -> String {
        let mut bridge = bridge_state().lock().expect("Python bridge mutex poisoned");
        let Some(snapshot) = &bridge.snapshot else {
            return "world bridge unavailable".to_owned();
        };

        if !snapshot.object_types.iter().any(|entry| entry == &type_id) {
            return format!("unknown object type: {type_id}");
        }

        bridge.spawn_requests.push(SpawnRequest { type_id, x, y });
        "queued".to_owned()
    }
}
