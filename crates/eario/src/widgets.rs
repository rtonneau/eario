//! The components a dashboard is made of.
//!
//! Each function returns a [`Widget`]: a component not yet placed on a page.
//! Pass widgets to [`Page::row`](crate::Page::row).

use eario_core::{ChartMark, ComponentKind, DataView, DatasetId, Filter};

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
