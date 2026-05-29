use std::cmp::Ordering;
use std::rc::Rc;
use yew::Html;

// ── Sort direction ────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Default)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

impl SortDir {
    pub fn toggle(&self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Asc => "↑",
            Self::Desc => "↓",
        }
    }
}

// ── Sort key — comparable value extracted from a row ─────────────────────────

#[derive(Clone)]
pub enum SortKey {
    Str(String),
    Num(f64),
    Nothing,
}

impl PartialEq for SortKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for SortKey {}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (SortKey::Str(a), SortKey::Str(b)) => a.cmp(b),
            (SortKey::Num(a), SortKey::Num(b)) => a.total_cmp(b),
            (SortKey::Nothing, SortKey::Nothing) => Ordering::Equal,
            // Nothing sorts last
            (SortKey::Nothing, _) => Ordering::Greater,
            (_, SortKey::Nothing) => Ordering::Less,
            // Mixed types: Str before Num
            (SortKey::Str(_), SortKey::Num(_)) => Ordering::Less,
            (SortKey::Num(_), SortKey::Str(_)) => Ordering::Greater,
        }
    }
}

// ── Column definition ─────────────────────────────────────────────────────────

/// Defines one column in a [`DataTable`].
///
/// `R` is the row data type. All function fields are `Rc<dyn Fn>` so they can
/// be cheaply cloned and compared by pointer for Yew's `PartialEq` check.
pub struct Column<R> {
    /// Stable identifier used for sort state, filter state, and column visibility.
    pub key: &'static str,
    /// Text shown in the column header.
    pub label: &'static str,
    /// Renders the cell content (not the `<td>` wrapper).
    pub render: Rc<dyn Fn(&R) -> Html>,
    /// Optional: returns a comparable key for client-side sorting.
    pub sort_value: Option<Rc<dyn Fn(&R) -> SortKey>>,
    /// Returns a plain-text representation for global search / per-column filter.
    pub search_value: Rc<dyn Fn(&R) -> String>,
    /// Optional CSS class applied to every `<td>` in this column.
    pub cell_class: Option<&'static str>,
    /// Whether the header renders a sort button.
    pub sortable: bool,
    /// Whether this column is shown by default (and in the col-toggle panel).
    pub default_visible: bool,
    /// Optional fixed width applied to the `<th>` via the `style` attribute.
    pub width: Option<&'static str>,
}

impl<R> Clone for Column<R> {
    fn clone(&self) -> Self {
        Self {
            key: self.key,
            label: self.label,
            render: Rc::clone(&self.render),
            sort_value: self.sort_value.as_ref().map(Rc::clone),
            search_value: Rc::clone(&self.search_value),
            cell_class: self.cell_class,
            sortable: self.sortable,
            default_visible: self.default_visible,
            width: self.width,
        }
    }
}

impl<R> PartialEq for Column<R> {
    /// Two columns are considered equal if they have the same key.
    /// This lets Yew skip re-renders when the parent rebuilds identical column vecs.
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
