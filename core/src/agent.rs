use crate::log::{see, Event, SessionLog};
use crate::sandbox::Sandbox;
use crate::types::{Error, Llm, Reply, Role, Tool, ToolSpec};


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutcome {
    pub text: String,
    pub steps: usize,
}








pub fn run_turn(
    llm: &mut dyn Llm,
    tools: &mut [Box<dyn Tool>],
    sandbox: &dyn Sandbox,
    log: &mut dyn SessionLog,
    user_input: &str,
    max_steps: usize,
) -> Result<TurnOutcome, Error> {
    let specs: Vec<ToolSpec> = tools.iter().map(|t| t.spec()).collect();
    let mut context = log.replay_context();
    see(log, &mut context, Role::User, user_input.to_string())?;

    for step in 1..=max_steps {
        let reply = match llm.complete(&context, &specs) {
            Ok(r) => r,
            Err(e) => {
                log.append(Event::Error(e.to_string()))?;
                return Err(e);
            }
        };
        match reply {
            Reply::Text(text) => {
                see(log, &mut context, Role::Assistant, text.clone())?;
                return Ok(TurnOutcome { text, steps: step });
            }
            Reply::Call(call) => {
                log.append(Event::Call(call.clone()))?;
                let result = match tools.iter_mut().find(|t| t.spec().name == call.name) {
                    Some(tool) => tool.call(&call.input, sandbox),
                    None => Err(Error::Tool(format!("unknown tool `{}`", call.name))),
                };
                let content = match result {
                    Ok(out) => out,
                    Err(e) => format!("ERROR: {e}"),
                };
                see(log, &mut context, Role::Tool, content)?;
            }
        }
    }
    let e = Error::StepLimit(max_steps);
    log.append(Event::Error(e.to_string()))?;
    Err(e)
}
