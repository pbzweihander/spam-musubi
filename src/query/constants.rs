use super::QueryOpMode;

#[derive(Debug, Clone)]
pub struct PreparedQueries {
	pub get_user: &'static str,
	pub get_instance_stats: &'static str,
}

pub fn get_prepared_queries(mode: QueryOpMode) -> PreparedQueries {
	match mode {
		QueryOpMode::Misskey => PreparedQueries {
			get_user: r#"SELECT t."followersCount", t."followingCount", t."notesCount" FROM public."user" t WHERE uri = $1 LIMIT 1"#,
			get_instance_stats: r#"SELECT "followersCount", "followingCount", "notesCount" FROM instance WHERE host = $1 LIMIT 1"#,
		},
		_ => unimplemented!(),
	}
}
