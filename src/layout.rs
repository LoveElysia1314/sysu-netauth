//! Logical grid tracks shared by native controls; DPI scaling happens after layout.
#[derive(Clone, Copy)]
pub(crate) struct Grid {
    pub bounds: [i32; 4],
    pub columns: usize,
    pub rows: usize,
    pub gap: [i32; 2],
}
impl Grid {
    pub fn cell(self, row: usize, column: usize) -> [i32; 4] {
        assert!(row < self.rows && column < self.columns);
        let [x, y, width, height] = self.bounds;
        let width = width - self.gap[0] * (self.columns as i32 - 1);
        let height = height - self.gap[1] * (self.rows as i32 - 1);
        assert!(width >= 0 && height >= 0);
        let left = width * column as i32 / self.columns as i32;
        let right = width * (column + 1) as i32 / self.columns as i32;
        let top = height * row as i32 / self.rows as i32;
        let bottom = height * (row + 1) as i32 / self.rows as i32;
        [
            x + left + self.gap[0] * column as i32,
            y + top + self.gap[1] * row as i32,
            right - left,
            bottom - top,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::Grid;
    #[test]
    fn columns_align_across_rows_and_panels() {
        let grid = Grid {
            bounds: [28, 296, 312, 84],
            columns: 2,
            rows: 3,
            gap: [8, 6],
        };
        for row in 0..3 {
            assert_eq!(grid.cell(row, 0)[0], 28);
            assert_eq!(grid.cell(row, 1)[0], 188);
            assert_eq!(grid.cell(row, 0)[2], grid.cell(row, 1)[2]);
        }
        let account = Grid {
            bounds: [28, 156, 312, 24],
            rows: 1,
            ..grid
        };
        assert_eq!(account.cell(0, 1)[0], grid.cell(2, 1)[0]);
    }
    #[test]
    fn three_button_columns_keep_the_empty_track() {
        let grid = Grid {
            bounds: [380, 332, 312, 60],
            columns: 3,
            rows: 2,
            gap: [8, 8],
        };
        let [x, _, width, _] = grid.cell(0, 1);
        assert!(x + width < grid.bounds[0] + grid.bounds[2]);
        assert_eq!(grid.cell(0, 1)[0], grid.cell(1, 1)[0]);
    }
    #[test]
    fn resizing_distributes_rounding_without_overflow_or_overlap() {
        for width in 300..800 {
            let grid = Grid {
                bounds: [0, 0, width, 60],
                columns: 3,
                rows: 2,
                gap: [8, 8],
            };
            let cells: Vec<_> = (0..3).map(|column| grid.cell(0, column)).collect();
            assert_eq!(cells[2][0] + cells[2][2], width);
            for pair in cells.windows(2) {
                assert_eq!(pair[1][0] - pair[0][0] - pair[0][2], 8);
                assert!((pair[1][2] - pair[0][2]).abs() <= 1);
            }
        }
    }
}
