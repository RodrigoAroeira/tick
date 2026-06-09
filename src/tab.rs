#[derive(Clone, Copy)]
pub enum Tab {
    Todos,
    Dones,
}

impl Tab {
    pub fn toggle(self) -> Self {
        match self {
            Tab::Todos => Tab::Dones,
            Tab::Dones => Tab::Todos,
        }
    }
}
