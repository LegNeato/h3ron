use crate::{Error, IndexChunked};
use h3ron::H3Cell;
use polars_core::prelude::{IntoSeries, ListChunked, UInt64Chunked};

#[inline]
pub(crate) fn list_map_cells<F>(cc: &IndexChunked<H3Cell>, map_fn: F) -> Result<ListChunked, Error>
where
    F: Fn(H3Cell) -> Result<UInt64Chunked, Error>,
{
    // todo: parallelize
    cc.iter_indexes_validated()
        .map(|opt| match opt {
            None => Ok(None),
            Some(Err(e)) => Err(e),
            Some(Ok(cell)) => map_fn(cell).map(|uc| Some(uc.into_series())),
        })
        .collect::<Result<ListChunked, _>>()
}
