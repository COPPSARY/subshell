use serde::Serialize;

// generic pagination DTO

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
}

impl<T> Page<T> {
    pub fn first(items: Vec<T>) -> Self {
        Self {
            total: items.len(),
            items,
            offset: 0,
            limit: 100,
        }
    }
}
