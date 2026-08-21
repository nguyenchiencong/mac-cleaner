pub mod size;
pub mod traversal;

pub use size::{calculate_path_size, calculate_path_size_with_diagnostics, SizeError};
pub use traversal::{
    detect_project_markers, find_project_root, is_git_repository, walk_directory_safely,
    ProjectMarker, ProjectMarkerType, TraversalConfig, TraversalResult,
};
