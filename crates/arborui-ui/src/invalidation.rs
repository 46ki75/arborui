/// Least expensive UI operation required after a change.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Invalidation {
    /// No visual work is required.
    #[default]
    None,
    /// Paint again using existing geometry.
    Paint,
    /// Recalculate geometry and then paint.
    Layout,
    /// Rebuild and reconcile the declarative structure.
    Recompose,
}

impl Invalidation {
    /// Escalates this request without allowing a cheaper request to replace it.
    pub fn request(&mut self, requested: Self) {
        *self = (*self).max(requested);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_only_escalate() {
        let mut invalidation = Invalidation::Paint;
        invalidation.request(Invalidation::Recompose);
        invalidation.request(Invalidation::Layout);

        assert_eq!(invalidation, Invalidation::Recompose);
    }
}
