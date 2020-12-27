use ndarray::{ArrayView2, Axis};
use geo_types::{Rect, Coordinate};
use crate::transform::Transform;
use crate::sphere::{area_rect, area_linearring};
use h3::index::Index;
use crate::error::Error;

fn find_continuous_chunks_along_axis<T>(a: &ArrayView2<T>, axis: usize, nodata_value: &T) -> Vec<(usize, usize)> where T: Sized + PartialEq {
    let mut chunks = Vec::new();
    let mut current_chunk_start: Option<usize> = None;

    for (r0pos, r0) in a.axis_iter(Axis(axis)).enumerate() {
        if r0.iter().any(|v| v != nodata_value) {
            if current_chunk_start.is_none() {
                current_chunk_start = Some(r0pos);
            }
        } else if let Some(begin) = current_chunk_start {
            chunks.push((begin, r0pos - 1));
            current_chunk_start = None;
        }
    }
    if let Some(begin) = current_chunk_start {
        chunks.push((begin, a.shape()[axis] - 1));
    }
    chunks
}

/// find all boxes in the array where there are any values except the nodata_value
///
/// this implementation is far from perfect and often recognizes multiple smaller
/// clusters as one as its based on completely empty columns and rows, but it is probably
/// sufficient for the purpose to reduce the number of hexagons
/// to be generated when dealing with fragmented/sparse datasets.
pub fn find_boxes_containing_data<T>(a: &ArrayView2<T>, nodata_value: &T) -> Vec<Rect<usize>> where T: Sized + PartialEq {
    let mut boxes = Vec::new();

    for chunk_0raw_indexes in find_continuous_chunks_along_axis(a, 0, nodata_value) {
        let sv = a.slice(s![chunk_0raw_indexes.0..=chunk_0raw_indexes.1, ..]);
        for chunks_1raw_indexes in find_continuous_chunks_along_axis(&sv, 1, nodata_value) {
            let sv2 = sv.slice(s![0..=(chunk_0raw_indexes.1-chunk_0raw_indexes.0), chunks_1raw_indexes.0..=chunks_1raw_indexes.1]);

            // one more iteration along axis 0 to get the specific range for that axis 1 range
            for chunks_0_indexes in find_continuous_chunks_along_axis(&sv2, 0, nodata_value) {
                boxes.push(Rect::new(
                    Coordinate {
                        x: chunks_0_indexes.0 + chunk_0raw_indexes.0,
                        y: chunks_1raw_indexes.0,
                    },
                    Coordinate {
                        x: chunks_0_indexes.1 + chunk_0raw_indexes.0,
                        y: chunks_1raw_indexes.1,
                    },
                ))
            }
        }
    }
    boxes
}

pub enum NearestH3ResolutionSearchMode {
    /// chose the h3 resolution where the difference in the area of a pixel and the h3index is
    /// as small as possible.
    SmallestAreaDifference,

    /// chose the h3 rsoulution where the area of the h3index is smaller than the area of a pixel.
    IndexAreaSmallerThanPixelArea,
}

/// find the h3 resolution closed to the size of a pixel in an array
/// of the given shape with the given transform
pub fn nearest_h3_resolution(shape: &[usize; 2], transform: &Transform, search_mode: NearestH3ResolutionSearchMode) -> Result<u8, Error> {
    if shape[0] == 0 || shape[1] == 0 {
        return Err(Error::EmptyArray);
    }
    let bbox_array = Rect::new(
        transform * &Coordinate::from((0.0_f64, 0.0_f64)),
        transform * &Coordinate::from((
            (shape[0] - 1) as f64,
            (shape[1] - 1) as f64
        )),
    );
    let area_pixel = area_rect(&bbox_array) / (shape[0] * shape[1]) as f64;
    let center_of_array = bbox_array.center();

    let mut nearest_h3_res = 0;
    let mut area_difference = None;
    for h3_res in 0..=16 {
        // calculate the area of the center index to avoid using the approximate values
        // of the h3 hexArea functions
        let area_h3_index = area_linearring(Index::from_coordinate(&center_of_array, h3_res)
            .polygon()
            .exterior());

        match search_mode {
            NearestH3ResolutionSearchMode::IndexAreaSmallerThanPixelArea => if area_h3_index <= area_pixel {
                nearest_h3_res = h3_res;
                break;
            }

            NearestH3ResolutionSearchMode::SmallestAreaDifference => {
                let new_area_difference = if area_h3_index > area_pixel {
                    area_h3_index - area_pixel
                } else {
                    area_pixel - area_h3_index
                };
                if let Some(old_area_difference) = area_difference {
                    if old_area_difference < new_area_difference {
                        nearest_h3_res = h3_res - 1;
                        break;
                    } else {
                        area_difference = Some(new_area_difference);
                    }
                } else {
                    area_difference = Some(new_area_difference);
                }
            }
        }
    }

    Ok(nearest_h3_res)
}

#[cfg(test)]
mod tests {
    use crate::algo::{find_boxes_containing_data, nearest_h3_resolution, NearestH3ResolutionSearchMode};
    use crate::transform::Transform;

    #[test]
    fn test_find_boxes_containig_data() {
        let arr = array![
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0],
            [0, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1],
            [0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 1],
        ];
        let mut arr_copy = arr.clone();

        let n_elements = arr_copy.shape()[0] * arr_copy.shape()[1];
        let mut n_elements_in_boxes = 0;

        for rect in find_boxes_containing_data(&arr.view(), &0) {
            n_elements_in_boxes += (rect.max().x - rect.min().x + 1) * (rect.max().y - rect.min().y + 1);

            //dbg!(rect);
            for x in rect.min().x..=rect.max().x {
                for y in rect.min().y..=rect.max().y {
                    arr_copy[(x, y)] = 0;
                }
            }
        }
        //dbg!(n_elements);
        //dbg!(n_elements_in_boxes);

        // there should be far less indexes to visit now
        assert!(n_elements_in_boxes < (n_elements / 2));

        // all elements should have been removed
        assert_eq!(arr_copy.sum(), 0);
    }

    #[test]
    fn test_nearest_h3_resolution() {
        // transform of the included r.tiff
        let gt = Transform::from_rasterio(&[
            0.0011965049999999992, 0.0, 8.11377, 0.0, -0.001215135, 49.40792
        ]);
        let h3_res1 = nearest_h3_resolution(&[2000_usize, 2000_usize], &gt, NearestH3ResolutionSearchMode::SmallestAreaDifference).unwrap();
        assert_eq!(h3_res1, 10); // TODO: validate

        let h3_res2 = nearest_h3_resolution(&[2000_usize, 2000_usize], &gt, NearestH3ResolutionSearchMode::IndexAreaSmallerThanPixelArea).unwrap();
        assert_eq!(h3_res2, 11); // TODO: validate
    }
}
