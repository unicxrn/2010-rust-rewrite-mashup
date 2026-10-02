//! Whether a controller is plugged in, for the status card: skating in
//! the game needs one.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Controller {
    /// The gamepad library couldn't start, so the launcher can't tell.
    Unavailable,
    /// No controller connected.
    None,
    /// `name` is the first connected controller; `count` counts them all.
    Connected { name: String, count: usize },
}

impl Controller {
    /// Sums up the connected controllers' names, in the order the gamepad
    /// library lists them.
    pub fn from_names<S: AsRef<str>>(names: impl IntoIterator<Item = S>) -> Self {
        let mut names = names.into_iter();
        let Some(first) = names.next() else {
            return Self::None;
        };
        let name = first.as_ref().trim();
        Self::Connected {
            name: if name.is_empty() {
                "Controller".to_owned()
            } else {
                name.to_owned()
            },
            count: 1 + names.count(),
        }
    }

    /// The controllers connected right now. gilrs keeps its own record of
    /// connections, updated as its events are drained.
    pub fn read(gilrs: Option<&gilrs::Gilrs>) -> Self {
        match gilrs {
            Some(gilrs) => Self::from_names(gilrs.gamepads().map(|(_, pad)| pad.name().to_owned())),
            None => Self::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_up_connected_pads() {
        assert_eq!(Controller::from_names([""; 0]), Controller::None);
        assert_eq!(
            Controller::from_names(["Xbox Wireless Controller"]),
            Controller::Connected {
                name: "Xbox Wireless Controller".into(),
                count: 1
            }
        );
        assert_eq!(
            Controller::from_names(["DualSense", "Xbox 360 Pad", "8BitDo"]),
            Controller::Connected {
                name: "DualSense".into(),
                count: 3
            }
        );
        assert_eq!(
            Controller::from_names(["  "]),
            Controller::Connected {
                name: "Controller".into(),
                count: 1
            }
        );
        assert_eq!(Controller::read(None), Controller::Unavailable);
    }
}
