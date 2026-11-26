//! The components a dashboard is made of.
//!
//! Each function returns a [`Widget`]: a component not yet placed on a page.
//! Pass widgets to [`Page::row`](crate::Page::row).

use std::ops::{Bound, RangeBounds, RangeInclusive};

use eario_core::{ChartMark, ComponentKind, DataView, DatasetId, Filter, SelectOptions};

use crate::state::State;

/// Number of points a chart receives at most, unless set otherwise.
const DEFAULT_MAX_POINTS: u32 = 5000;

/// Number of rows a table shows per page, unless set otherwise.
const DEFAULT_PAGE_SIZE: u32 = 50;

/// A component and its presentation options, before it gets an identifier
/// and a place in the layout.
#[derive(Debug, Clone)]
pub struct Widget {
    pub(crate) id: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) span: Option<u8>,
    pub(crate) kind: ComponentKind,
}

impl Widget {
    fn new(kind: ComponentKind) -> Self {
        Self { id: None, title: None, span: None, kind }
    }

    /// Sets the identifier of the component. Without one, an identifier is
    /// derived from the kind and position of the component. Set it when
    /// something outside the dashboard, such as a saved session or an agent,
    /// needs a stable name.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the title shown above the component.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the width of the component, in columns of the 12-column grid.
    /// Without a span, the components of a row share the free columns evenly.
    pub fn span(mut self, columns: u8) -> Self {
        self.span = Some(columns);
        self
    }

    /// Sets the largest number of points a chart sends to the browser.
    /// Has no effect on other components.
    pub fn max_points(mut self, limit: u32) -> Self {
        if let ComponentKind::Chart { max_points, .. } = &mut self.kind {
            *max_points = limit;
        }
        self
    }

    /// Sets the number of rows a table shows per page.
    /// Has no effect on other components.
    pub fn page_size(mut self, rows: u32) -> Self {
        if let ComponentKind::DataTable { page_size, .. } = &mut self.kind {
            *page_size = rows;
        }
        self
    }

    /// Sets the distance between two accepted values of a number input or
    /// a slider, counted from its minimum (from 0 for a number input
    /// without one). The default value must be one of them, and an integer
    /// input needs a whole step; [`Dashboard::build`](crate::Dashboard::build)
    /// reports a step that does not fit. Has no effect on other components.
    pub fn step(mut self, distance: f64) -> Self {
        match &mut self.kind {
            ComponentKind::NumberInput { step, .. } => *step = Some(distance),
            ComponentKind::Slider { step, .. } => *step = distance,
            _ => {}
        }
        self
    }

    /// Sets the largest number of characters a text input accepts.
    /// Has no effect on other components.
    pub fn max_length(mut self, characters: u32) -> Self {
        if let ComponentKind::TextInput { max_length, .. } = &mut self.kind {
            *max_length = characters;
        }
        self
    }
}

mod sealed {
    /// Keeps [`NumberType`](super::NumberType) implemented by the types
    /// listed there only.
    pub trait Sealed {}
}

/// A type of number a [`number_input`] or a [`slider`] writes: `i64`, `u32`
/// or `f64`.
///
/// It decides whether the input accepts whole numbers only, and the bounds
/// it has whatever the developer sets: a `u32` input accepts whole numbers
/// from 0 to `u32::MAX`. The trait is sealed: it cannot be implemented
/// outside this crate.
pub trait NumberType: sealed::Sealed + Copy {
    /// Whether the input accepts whole numbers only.
    const INTEGER: bool;
    /// Smallest value the type holds, when the input must enforce it.
    const MIN: Option<f64>;
    /// Largest value the type holds, when the input must enforce it.
    const MAX: Option<f64>;

    /// The value as the specification stores it.
    fn to_f64(self) -> f64;

    /// The largest accepted value below `self`, for an excluded upper bound.
    fn below(self) -> f64;

    /// The smallest accepted value above `self`, for an excluded lower bound.
    fn above(self) -> f64;
}

impl sealed::Sealed for i64 {}
impl sealed::Sealed for u32 {}
impl sealed::Sealed for f64 {}

/// Whole numbers. Values beyond ±2^53 cannot be used as bounds or default:
/// the specification stores them as `f64`.
impl NumberType for i64 {
    const INTEGER: bool = true;
    const MIN: Option<f64> = None;
    const MAX: Option<f64> = None;

    fn to_f64(self) -> f64 {
        // Exact within ±2^53; validation rejects bounds beyond.
        self as f64
    }

    fn below(self) -> f64 {
        self.to_f64() - 1.0
    }

    fn above(self) -> f64 {
        self.to_f64() + 1.0
    }
}

/// Whole numbers from 0 to `u32::MAX`.
impl NumberType for u32 {
    const INTEGER: bool = true;
    const MIN: Option<f64> = Some(0.0);
    const MAX: Option<f64> = Some(u32::MAX as f64);

    fn to_f64(self) -> f64 {
        f64::from(self)
    }

    fn below(self) -> f64 {
        self.to_f64() - 1.0
    }

    fn above(self) -> f64 {
        self.to_f64() + 1.0
    }
}

/// Any finite number.
impl NumberType for f64 {
    const INTEGER: bool = false;
    const MIN: Option<f64> = None;
    const MAX: Option<f64> = None;

    fn to_f64(self) -> f64 {
        self
    }

    fn below(self) -> f64 {
        self.next_down()
    }

    fn above(self) -> f64 {
        self.next_up()
    }
}

/// The bounds of `range` as the specification stores them; an unbounded
/// side takes the bound of the type, if it has one. A bound given as a `T`
/// is within those of the type.
fn number_bounds<T: NumberType>(range: &impl RangeBounds<T>) -> (Option<f64>, Option<f64>) {
    let min = match range.start_bound() {
        Bound::Included(start) => Some(start.to_f64()),
        Bound::Excluded(start) => Some(start.above()),
        Bound::Unbounded => None,
    };
    let max = match range.end_bound() {
        Bound::Included(end) => Some(end.to_f64()),
        Bound::Excluded(end) => Some(end.below()),
        Bound::Unbounded => None,
    };
    (min.or(T::MIN), max.or(T::MAX))
}

/// Static text. Blank lines separate paragraphs.
pub fn text(text: impl Into<String>) -> Widget {
    Widget::new(ComponentKind::Text { text: text.into() })
}

/// Lets the user load a CSV, TSV or XLSX file into `dataset` and, for
/// workbooks, choose the worksheet.
pub fn file_import(dataset: &DatasetId) -> Widget {
    Widget::new(ComponentKind::FileImport { dataset: dataset.clone() }).title("Data source")
}

/// Shows the source, size and columns of `dataset`.
pub fn dataset_info(dataset: &DatasetId) -> Widget {
    Widget::new(ComponentKind::DatasetInfo { dataset: dataset.clone() }).title("Dataset")
}

/// Lets the user choose any column of `dataset`; the name of the column is
/// stored in `state`. The first column is chosen when a dataset is loaded.
pub fn column_select(
    label: impl Into<String>,
    dataset: &DatasetId,
    state: &State<String>,
) -> Widget {
    Widget::new(ComponentKind::ColumnSelect {
        dataset: dataset.clone(),
        state: state.key().clone(),
        label: label.into(),
        numeric_only: false,
        default_index: 0,
    })
}

/// Lets the user choose a numeric column of `dataset`; the name of the
/// column is stored in `state`. When a dataset is loaded, the numeric column
/// at position `default_index` (counting from 0) is chosen.
pub fn numeric_column_select(
    label: impl Into<String>,
    dataset: &DatasetId,
    state: &State<String>,
    default_index: u32,
) -> Widget {
    Widget::new(ComponentKind::ColumnSelect {
        dataset: dataset.clone(),
        state: state.key().clone(),
        label: label.into(),
        numeric_only: true,
        default_index,
    })
}

/// Lets the user filter the rows of `dataset` on one column; the filter is
/// stored in `state`. Apply it to the rows of a component or of a
/// computation input with [`filtered`](crate::filtered):
/// `data_table(filtered(&data, &state))`.
pub fn filter_input(dataset: &DatasetId, state: &State<Filter>) -> Widget {
    Widget::new(ComponentKind::FilterInput { dataset: dataset.clone(), state: state.key().clone() })
        .title("Filter")
}

/// A paged table of the rows of `view`.
pub fn data_table(view: impl Into<DataView>) -> Widget {
    Widget::new(ComponentKind::DataTable { view: view.into(), page_size: DEFAULT_PAGE_SIZE })
        .title("Rows")
}

/// Descriptive statistics of the numeric columns of `view`.
pub fn statistics(view: impl Into<DataView>) -> Widget {
    Widget::new(ComponentKind::Statistics { view: view.into() }).title("Statistics")
}

fn chart(mark: ChartMark, view: DataView, x: &State<String>, y: &State<String>) -> Widget {
    Widget::new(ComponentKind::Chart {
        view,
        mark,
        x: x.key().clone(),
        y: y.key().clone(),
        max_points: DEFAULT_MAX_POINTS,
    })
}

/// A scatter plot of the rows of `view`. The columns on the axes are the
/// ones named by the states `x` and `y`.
pub fn scatter_chart(view: impl Into<DataView>, x: &State<String>, y: &State<String>) -> Widget {
    chart(ChartMark::Scatter, view.into(), x, y)
}

/// A line chart of the rows of `view`, connected in row order. The columns
/// on the axes are the ones named by the states `x` and `y`.
pub fn line_chart(view: impl Into<DataView>, x: &State<String>, y: &State<String>) -> Widget {
    chart(ChartMark::Line, view.into(), x, y)
}

/// Number of characters a text input accepts, unless set otherwise.
const DEFAULT_MAX_TEXT_LENGTH: u32 = 1_000;

/// Number of steps a slider of decimal numbers has, unless set otherwise.
const DEFAULT_SLIDER_STEPS: f64 = 100.0;

/// Lets the user type a number within `range`; the number is stored in
/// `state`, which starts at `default`.
///
/// The range may be open on either side: `1..=200`, `0.0..` or `..`. A
/// `State<i64>` or `State<u32>` accepts whole numbers only, and a
/// `State<u32>` none below 0 or above `u32::MAX`. Restrict the accepted
/// values further with [`Widget::step`].
///
/// ```
/// use eario::prelude::*;
///
/// let bins = State::<u32>::new("bins");
/// let input = number_input("Bins", &bins, 1..=200, 20);
/// ```
pub fn number_input<T: NumberType>(
    label: impl Into<String>,
    state: &State<T>,
    range: impl RangeBounds<T>,
    default: T,
) -> Widget {
    let (min, max) = number_bounds(&range);
    Widget::new(ComponentKind::NumberInput {
        label: label.into(),
        state: state.key().clone(),
        min,
        max,
        step: None,
        integer: T::INTEGER,
        default: default.to_f64(),
    })
}

/// Lets the user choose a number within `range` by dragging; the number is
/// stored in `state`, which starts at `default`.
///
/// A slider of whole numbers moves by 1, one of decimal numbers by a
/// hundredth of the range; change it with [`Widget::step`].
///
/// ```
/// use eario::prelude::*;
///
/// let window = State::<u32>::new("window");
/// let smoothing = slider("Moving average window", &window, 1..=50, 5);
/// let threshold = State::<f64>::new("threshold");
/// let cut = slider("Threshold", &threshold, 0.0..=1.0, 0.5).step(0.05);
/// ```
pub fn slider<T: NumberType>(
    label: impl Into<String>,
    state: &State<T>,
    range: RangeInclusive<T>,
    default: T,
) -> Widget {
    let (min, max) = (range.start().to_f64(), range.end().to_f64());
    let step = if T::INTEGER { 1.0 } else { (max - min) / DEFAULT_SLIDER_STEPS };
    Widget::new(ComponentKind::Slider {
        label: label.into(),
        state: state.key().clone(),
        min,
        max,
        step,
        integer: T::INTEGER,
        default: default.to_f64(),
    })
}

/// Lets the user type a line of text; the text is stored in `state`, which
/// starts as `default`. At most 1,000 characters are accepted; change it
/// with [`Widget::max_length`].
pub fn text_input(
    label: impl Into<String>,
    state: &State<String>,
    default: impl Into<String>,
) -> Widget {
    Widget::new(ComponentKind::TextInput {
        label: label.into(),
        state: state.key().clone(),
        default: default.into(),
        max_length: DEFAULT_MAX_TEXT_LENGTH,
    })
}

/// Lets the user choose one of `options`; the choice is stored in `state`,
/// which starts as `default`. The default must be one of the options.
///
/// ```
/// use eario::prelude::*;
///
/// let unit = State::<String>::new("unit");
/// let input = select("Unit", &unit, ["keV", "MeV"], "keV");
/// ```
pub fn select<S: Into<String>>(
    label: impl Into<String>,
    state: &State<String>,
    options: impl IntoIterator<Item = S>,
    default: impl Into<String>,
) -> Widget {
    Widget::new(ComponentKind::Select {
        label: label.into(),
        state: state.key().clone(),
        options: SelectOptions::Static(options.into_iter().map(Into::into).collect()),
        default: Some(default.into()),
    })
}

/// Lets the user choose one of the distinct values of `column` in
/// `dataset`, as text, sorted; the choice is stored in `state`.
///
/// The first value is chosen when a dataset is loaded, and again when the
/// dataset changes and no longer holds the chosen one. At most the first
/// 1,000 values are offered; the select says when there are more.
pub fn select_from_column(
    label: impl Into<String>,
    dataset: &DatasetId,
    column: impl Into<String>,
    state: &State<String>,
) -> Widget {
    Widget::new(ComponentKind::Select {
        label: label.into(),
        state: state.key().clone(),
        options: SelectOptions::Column { dataset: dataset.clone(), column: column.into() },
        default: None,
    })
}

/// Lets the user switch an option on or off; `true` or `false` is stored in
/// `state`, which starts as `default`.
pub fn checkbox(label: impl Into<String>, state: &State<bool>, default: bool) -> Widget {
    Widget::new(ComponentKind::Checkbox {
        label: label.into(),
        state: state.key().clone(),
        default,
    })
}

#[cfg(test)]
mod tests {
    use eario_core::{SpecError, StateKey};

    use super::*;

    #[test]
    fn number_inputs_take_bounds_and_integrality_from_the_type() {
        let bins = State::<u32>::new("bins");
        assert_eq!(
            number_input("Bins", &bins, .., 20).kind,
            ComponentKind::NumberInput {
                label: "Bins".to_owned(),
                state: StateKey::new("bins"),
                min: Some(0.0),
                max: Some(f64::from(u32::MAX)),
                step: None,
                integer: true,
                default: 20.0,
            }
        );
        let offset = State::<i64>::new("offset");
        assert!(matches!(
            number_input("Offset", &offset, -10.., 0).kind,
            ComponentKind::NumberInput { min: Some(-10.0), max: None, integer: true, .. }
        ));
        assert!(matches!(
            number_input("Offset", &offset, 0..10, 0).kind,
            ComponentKind::NumberInput { max: Some(9.0), .. }
        ));

        let scale = State::<f64>::new("scale");
        let decimal = number_input("Scale", &scale, 0.0..1.0, 0.5).step(0.25).kind;
        let ComponentKind::NumberInput { min, max, step, integer, .. } = decimal else {
            panic!("not a number input: {decimal:?}");
        };
        assert_eq!((min, step, integer), (Some(0.0), Some(0.25), false));
        assert!(max.is_some_and(|max| max < 1.0 && max > 0.999), "{max:?}");
    }

    #[test]
    fn sliders_step_by_one_or_by_a_hundredth_of_the_range() {
        let window = State::<u32>::new("window");
        assert!(matches!(
            slider("Window", &window, 1..=50, 5).kind,
            ComponentKind::Slider {
                min: 1.0,
                max: 50.0,
                step: 1.0,
                integer: true,
                default: 5.0,
                ..
            }
        ));
        let threshold = State::<f64>::new("threshold");
        assert!(matches!(
            slider("Threshold", &threshold, 0.0..=2.0, 1.0).kind,
            ComponentKind::Slider { step: 0.02, integer: false, .. }
        ));
        assert!(matches!(
            slider("Threshold", &threshold, 0.0..=2.0, 1.0).step(0.5).kind,
            ComponentKind::Slider { step: 0.5, .. }
        ));
    }

    #[test]
    fn text_select_and_checkbox_inputs_hold_their_defaults() {
        let title = State::<String>::new("title");
        assert!(matches!(
            text_input("Title", &title, "Run").max_length(12).kind,
            ComponentKind::TextInput { max_length: 12, ref default, .. } if default == "Run"
        ));
        assert_eq!(
            select("Unit", &title, ["keV", "MeV"], "MeV").kind,
            ComponentKind::Select {
                label: "Unit".to_owned(),
                state: StateKey::new("title"),
                options: SelectOptions::Static(vec!["keV".to_owned(), "MeV".to_owned()]),
                default: Some("MeV".to_owned()),
            }
        );
        let data = DatasetId::new("data");
        assert!(matches!(
            select_from_column("Sample", &data, "sample", &title).kind,
            ComponentKind::Select {
                options: SelectOptions::Column { ref column, .. },
                default: None,
                ..
            } if column == "sample"
        ));
        let outliers = State::<bool>::new("outliers");
        assert!(matches!(
            checkbox("Outliers", &outliers, true).kind,
            ComponentKind::Checkbox { default: true, .. }
        ));
    }

    #[test]
    fn a_step_that_does_not_fit_an_integer_input_is_rejected() {
        let bins = State::<u32>::new("bins");
        let spec = crate::Dashboard::new("Steps")
            .page("Main", |page| page.row([number_input("Bins", &bins, 1..=10, 2).step(0.5)]))
            .spec();
        let error = spec.validate();
        assert!(
            matches!(
                &error,
                Err(SpecError::InvalidInput { reason, .. })
                    if reason.contains("step 0.5 is not a whole number")
            ),
            "{error:?}"
        );
    }
}
