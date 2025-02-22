use crate::algorithm::bounding_rect::BoundingRect;
use crate::spatial_index::{finish_mask, negative_mask, RectIndexable, RectSIKind, SpatialIndex};
use crate::{AsH3IndexChunked, Error, IndexChunked, IndexValue};
use geo_types::{coord, Coordinate, Rect};
use polars::export::arrow::bitmap::MutableBitmap;
use polars::prelude::{BooleanChunked, UInt64Chunked};
use static_aabb2d_index::{NeighborVisitor, StaticAABB2DIndex, StaticAABB2DIndexBuilder};
use std::marker::PhantomData;

/// Spatial index implementation using the [packed Hilbert R-tree](https://en.wikipedia.org/wiki/Hilbert_R-tree#Packed_Hilbert_R-trees) algorithm
///
/// Based on [flatbush](https://github.com/mourner/flatbush) and the rust port [static_aabb2d_index](https://github.com/jbuckmccready/static_aabb2d_index).
///
/// # Example
///
/// ```
/// use polars::prelude::UInt64Chunked;
/// use h3ron::H3Cell;
/// use h3ron_polars::{AsH3CellChunked, NamedFromIndexes};
/// use h3ron_polars::spatial_index::BuildPackedHilbertRTreeIndex;
///
/// let uc = UInt64Chunked::new_from_indexes(
///     "",
///     vec![
///         H3Cell::from_coordinate((45.5, 45.5).into(), 7).unwrap(),
///         H3Cell::from_coordinate((-60.5, -60.5).into(), 7).unwrap(),
///         H3Cell::from_coordinate((120.5, 70.5).into(), 7).unwrap(),
///     ],
/// );
///
/// let idx = uc.h3cell().packed_hilbert_rtree_index();
/// ```
pub struct PackedHilbertRTreeIndex<IX: IndexValue> {
    pub index: Option<StaticAABB2DIndex<f64>>,
    index_phantom: PhantomData<IX>,
    chunked_array: UInt64Chunked,

    /// maps the positions of the index contents to the position in the `chunked_array`
    positions_in_chunked_array: Box<[usize]>,
}

pub trait BuildPackedHilbertRTreeIndex<IX: IndexValue> {
    /// Build a spatial index using the [packed Hilbert R-tree](https://en.wikipedia.org/wiki/Hilbert_R-tree#Packed_Hilbert_R-trees) algorithm
    ///
    /// Based on [flatbush](https://github.com/mourner/flatbush) and the rust port [static_aabb2d_index](https://github.com/jbuckmccready/static_aabb2d_index).
    fn packed_hilbert_rtree_index(&self) -> Result<PackedHilbertRTreeIndex<IX>, Error>;
}

impl<'a, IX: IndexValue> BuildPackedHilbertRTreeIndex<IX> for IndexChunked<'a, IX>
where
    IX: RectIndexable,
{
    fn packed_hilbert_rtree_index(&self) -> Result<PackedHilbertRTreeIndex<IX>, Error> {
        let (positions_in_chunked_array, rects) = self.iter_indexes_validated().enumerate().fold(
            (
                Vec::with_capacity(self.len()),
                Vec::with_capacity(self.len()),
            ),
            |(mut positions, mut rects), (pos, maybe_index)| {
                if let Some(Ok(index)) = maybe_index {
                    if let Ok(Some(rect)) = index.spatial_index_rect() {
                        positions.push(pos);
                        rects.push(rect)
                    }
                }
                (positions, rects)
            },
        );

        let index = if !positions_in_chunked_array.is_empty() {
            let mut builder = StaticAABB2DIndexBuilder::new(positions_in_chunked_array.len());
            for rect in rects {
                // add takes in (min_x, min_y, max_x, max_y) of the bounding box
                builder.add(rect.min().x, rect.min().y, rect.max().x, rect.max().y);
            }
            Some(
                builder
                    .build()
                    .map_err(|e| Error::SpatialIndex(e.to_string()))?,
            )
        } else {
            None
        };
        Ok(PackedHilbertRTreeIndex {
            index,
            index_phantom: PhantomData::<IX>::default(),
            chunked_array: self.chunked_array.clone(),
            positions_in_chunked_array: positions_in_chunked_array.into_boxed_slice(),
        })
    }
}

impl<IX: IndexValue> SpatialIndex<IX, RectSIKind> for PackedHilbertRTreeIndex<IX> {
    fn h3indexchunked(&self) -> IndexChunked<IX> {
        self.chunked_array.h3indexchunked()
    }

    fn envelopes_intersect_impl(&self, rect: &Rect) -> MutableBitmap {
        let mut mask = negative_mask(&self.chunked_array);
        if let Some(index) = self.index.as_ref() {
            for index_position in
                index.query(rect.min().x, rect.min().y, rect.max().x, rect.max().y)
            {
                mask.set(self.positions_in_chunked_array[index_position], true);
            }
        }
        mask
    }

    fn envelopes_within_distance(&self, coord: Coordinate, distance: f64) -> BooleanChunked {
        let mut mask = negative_mask(&self.chunked_array);

        if let Some(index) = self.index.as_ref() {
            let mut visitor = Visitor {
                found: vec![],
                distance,
            };
            index.visit_neighbors(coord.x, coord.y, &mut visitor);

            for index_position in visitor.found {
                mask.set(self.positions_in_chunked_array[index_position], true);
            }
        }

        finish_mask(mask.into(), &self.h3indexchunked())
    }
}

impl<IX: IndexValue> BoundingRect for PackedHilbertRTreeIndex<IX> {
    fn bounding_rect(&self) -> Result<Option<Rect>, Error> {
        if let Some(index) = self.index.as_ref() {
            Ok(Some(Rect::new(
                coord! {x: index.min_x(), y: index.min_y()},
                coord! {x: index.max_x(), y: index.max_y()},
            )))
        } else {
            Ok(None)
        }
    }
}

struct Visitor {
    found: Vec<usize>,
    distance: f64,
}

impl NeighborVisitor<f64, Result<(), ()>> for Visitor {
    fn visit(&mut self, index_pos: usize, dist_squared: f64) -> Result<(), ()> {
        if dist_squared <= self.distance {
            self.found.push(index_pos);
            Ok(())
        } else {
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::spatial_index::packed_hilbert_rtree::BuildPackedHilbertRTreeIndex;
    use crate::spatial_index::PackedHilbertRTreeIndex;
    use crate::IndexChunked;
    use h3ron::H3Cell;

    fn build_index(cc: &IndexChunked<H3Cell>) -> PackedHilbertRTreeIndex<H3Cell> {
        cc.packed_hilbert_rtree_index().unwrap()
    }
    crate::spatial_index::tests::impl_std_tests!(build_index);
}
