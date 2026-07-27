//! Department repository: record lifecycle and dashboard rollups.

mod crud;
mod summaries;

pub use crud::{
    assign_user_to_department, create_department, delete_department, find_department_by_name,
    update_department,
};
pub use summaries::list_departments;
