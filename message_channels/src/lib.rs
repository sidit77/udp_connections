
pub struct MessageChannel {

}

impl MessageChannel {

    pub fn new() -> Self {
        Self {}
    }

    pub fn read(&mut self, data: &[u8]) -> Vec<Box<[u8]>> {
        vec![data.into()]
    }

    pub fn send(&mut self, data: &[u8]) -> Box<[u8]> {
        data.into()
    }

}