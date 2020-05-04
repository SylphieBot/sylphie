use crate::errors::*;
use crate::interface::InterfaceShared;
use linefeed::{
    Interface as LinefeedInterface, DefaultTerminal, Signal, ReadResult, Writer,
};
use static_events::*;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::*;

pub struct TerminalCommandEvent(pub String);
simple_event!(TerminalCommandEvent);

pub struct TerminalLock<'a, 'b>(Writer<'a, 'b, DefaultTerminal>);

struct TerminalInfo {
    shared: Arc<InterfaceShared>,
    interface: LinefeedInterface<DefaultTerminal>,
}
pub struct Terminal(Arc<TerminalInfo>);
impl Terminal {
    pub(in super) fn new(shared: Arc<InterfaceShared>) -> Result<Terminal> {
        let internal_name = shared.info.bot_name.to_lowercase().replace(' ', "-");
        let interface = LinefeedInterface::new(internal_name.clone())?;
        interface.set_report_signal(Signal::Interrupt, true);
        interface.set_report_signal(Signal::Quit, true);
        interface.set_history_size(100);
        interface.set_prompt(&format!("{}> ", internal_name))?;
        Ok(Terminal(Arc::new(TerminalInfo { shared, interface })))
    }
    fn shutdown_msg(&self) -> Result<()> {
        write!(
            self.0.interface,
            "Please use the '.shutdown' command to stop {}.\n",
            self.0.shared.info.bot_name,
        )?;
        Ok(())
    }
    pub fn start_terminal(&self, target: &Handler<impl Events>) -> Result<()> {
        let mut last_failed = false;
        'outer: loop {
            let result = self.0.interface.read_line_step(Some(Duration::from_millis(100)));
            if result.is_ok() {
                last_failed = false;
            }
            match result {
                Ok(Some(ReadResult::Input(line))) => if !line.trim().is_empty() {
                    self.0.interface.add_history_unique(line.clone());

                    // TODO: Error reporting.
                    target.dispatch(TerminalCommandEvent(line));
                }
                Ok(Some(ReadResult::Eof)) => {
                    self.shutdown_msg()?;
                }
                Ok(Some(ReadResult::Signal(Signal::Interrupt))) => {
                    eprint!("^C\n");
                    self.shutdown_msg()?;
                    self.0.interface.set_buffer("")?;
                }
                Ok(Some(ReadResult::Signal(Signal::Quit))) => {
                    write!(self.0.interface, " (killed)\n")?;
                    break 'outer;
                }
                Ok(Some(ReadResult::Signal(sig))) =>
                    error!("Terminal reader received unexpected signal: {:?}", sig),
                Ok(None) => { }
                Err(err) => {
                    error!("Terminal reader encountered error: {}", err);
                    if last_failed {
                        error!("Terminal reader failed twice in a row. Exiting.");
                        break 'outer;
                    } else {
                        last_failed = true;
                    }
                },
            }
            if self.0.shared.is_shutdown.load(Ordering::Relaxed) {
                self.0.interface.cancel_read_line()?;
                break 'outer;
            }
        }
        Ok(())
    }
    pub fn lock_write(&self) -> Result<TerminalLock> {
        Ok(TerminalLock(self.0.interface.lock_writer_erase()?))
    }
}
