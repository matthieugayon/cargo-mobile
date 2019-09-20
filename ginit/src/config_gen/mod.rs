mod default;
mod domain_blacklist;

pub use self::default::*;
use crate::{
    config::app_name,
    ios,
    templating::template_pack,
    util::{self, prompt},
};
use colored::*;
use heck::TitleCase as _;
use std::{fmt, io, path::Path};

#[derive(Debug)]
pub enum InteractiveError {
    DefaultConfigDetectionFailed(DetectionError),
    AppNamePromptFailed(io::Error),
    StylizedAppNamePromptFailed(io::Error),
    DomainPromptFailed(io::Error),
    DeveloperTeamLookupFailed(ios::teams::Error),
    DeveloperTeamPromptFailed(io::Error),
}

impl fmt::Display for InteractiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InteractiveError::DefaultConfigDetectionFailed(err) => {
                write!(f, "Failed to detect default config values: {}", err)
            }
            InteractiveError::AppNamePromptFailed(err) => {
                write!(f, "Failed to prompt for app name: {}", err)
            }
            InteractiveError::StylizedAppNamePromptFailed(err) => {
                write!(f, "Failed to prompt for stylized app name: {}", err)
            }
            InteractiveError::DomainPromptFailed(err) => {
                write!(f, "Failed to prompt for domain: {}", err)
            }
            InteractiveError::DeveloperTeamLookupFailed(err) => {
                write!(f, "Failed to find Apple developer teams: {}", err)
            }
            InteractiveError::DeveloperTeamPromptFailed(err) => {
                write!(f, "Failed to prompt for Apple developer team: {}", err)
            }
        }
    }
}

#[derive(Debug)]
pub enum WriteError {
    ConfigTemplateMissing,
    ConfigRenderFailed(bicycle::ProcessingError),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::ConfigTemplateMissing => {
                write!(f, "Missing \"{}.toml\" template.", crate::NAME)
            }
            WriteError::ConfigRenderFailed(err) => {
                write!(f, "Failed to render config file: {}", err)
            }
        }
    }
}

#[derive(Debug)]
pub struct RequiredConfig {
    app_name: String,
    stylized_app_name: String,
    domain: String,
    development_team: String,
}

impl RequiredConfig {
    fn prompt_app_name(
        wrapper: &util::TextWrapper,
        defaults: &DefaultConfig,
    ) -> Result<(String, Option<String>), InteractiveError> {
        let mut default_app_name = defaults.app_name.clone();
        let mut app_name = None;
        let mut rejected = None;
        let mut default_stylized = None;
        while let None = app_name {
            let response = prompt::default(
                "App name",
                default_app_name.as_ref().map(|s| s.as_str()),
                None,
            )
            .map_err(InteractiveError::AppNamePromptFailed)?;
            match app_name::validate(response.clone()) {
                Ok(response) => {
                    if default_app_name == Some(response.clone()) {
                        if rejected.is_some() {
                            default_stylized = rejected.take();
                        } else {
                            default_stylized = Some(defaults.stylized_app_name.clone());
                        }
                    }
                    app_name = Some(response);
                }
                Err(err) => {
                    rejected = Some(response);
                    println!(
                        "{}",
                        wrapper
                            .fill(&format!("Gosh, that's not a valid app name! {}", err))
                            .bright_magenta()
                    );
                    if let Some(suggested) = err.suggested() {
                        default_app_name = Some(suggested.to_owned());
                    }
                }
            }
        }
        Ok((app_name.unwrap(), default_stylized))
    }

    fn prompt_stylized_app_name(
        app_name: &str,
        default_stylized: Option<String>,
    ) -> Result<String, InteractiveError> {
        let stylized = default_stylized
            .unwrap_or_else(|| app_name.replace("-", " ").replace("_", " ").to_title_case());
        prompt::default("Stylized app name", Some(&stylized), None)
            .map_err(InteractiveError::StylizedAppNamePromptFailed)
    }

    fn prompt_domain(
        wrapper: &util::TextWrapper,
        defaults: &DefaultConfig,
    ) -> Result<String, InteractiveError> {
        let mut domain = None;
        while let None = domain {
            let response = prompt::default("Domain", Some(&defaults.domain), None)
                .map_err(InteractiveError::DomainPromptFailed)?;
            if publicsuffix::Domain::has_valid_syntax(&response) {
                domain = Some(response);
            } else {
                println!(
                    "{}",
                    wrapper
                        .fill(&format!(
                            "Sorry, but {:?} isn't valid domain syntax.",
                            response
                        ))
                        .bright_magenta()
                );
            }
        }
        Ok(domain.unwrap())
    }

    fn prompt_development_team(wrapper: &util::TextWrapper) -> Result<String, InteractiveError> {
        let development_teams = ios::teams::find_development_teams()
            .map_err(InteractiveError::DeveloperTeamLookupFailed)?;
        let mut default_team = None;
        println!("Detected development teams:");
        for (index, team) in development_teams.iter().enumerate() {
            println!(
                "  [{}] {} ({})",
                index.to_string().green(),
                team.name,
                team.id.cyan(),
            );
            if development_teams.len() == 1 {
                default_team = Some("0");
            }
        }
        if development_teams.is_empty() {
            println!("  -- none --");
        }
        let mut development_team = None;
        while let None = development_team {
            println!(
                "  Enter an {} for a team above, or enter a {} manually.",
                "index".green(),
                "team ID".cyan(),
            );
            let team_input =
                prompt::default("Apple development team", default_team, Some(Color::Green))
                    .map_err(InteractiveError::DeveloperTeamPromptFailed)?;
            let team_id = team_input
                .parse::<usize>()
                .ok()
                .and_then(|index| development_teams.get(index))
                .map(|team| team.id.clone())
                .unwrap_or_else(|| team_input);
            if !team_id.is_empty() {
                development_team = Some(team_id);
            } else {
                println!(
                    "{}",
                    wrapper
                        .fill("Uh-oh, you need to specify a development team ID.")
                        .bright_magenta()
                );
            }
        }
        Ok(development_team.unwrap())
    }

    pub fn interactive(wrapper: &util::TextWrapper) -> Result<Self, InteractiveError> {
        let defaults =
            DefaultConfig::detect().map_err(InteractiveError::DefaultConfigDetectionFailed)?;
        let (app_name, default_stylized) = Self::prompt_app_name(wrapper, &defaults)?;
        let stylized_app_name = Self::prompt_stylized_app_name(&app_name, default_stylized)?;
        let domain = Self::prompt_domain(wrapper, &defaults)?;
        let development_team = Self::prompt_development_team(wrapper)?;
        Ok(Self {
            app_name,
            stylized_app_name,
            domain,
            development_team,
        })
    }

    pub fn write(
        self,
        bike: &bicycle::Bicycle,
        project_root: impl AsRef<Path>,
    ) -> Result<(), WriteError> {
        bike.process(
            template_pack(None, "{{tool-name}}.toml.hbs")
                .ok_or_else(|| WriteError::ConfigTemplateMissing)?,
            project_root,
            |map| {
                map.insert("app-name", &self.app_name);
                map.insert("stylized-app-name", &self.stylized_app_name);
                map.insert("domain", &self.domain);
                map.insert("development-team", &self.development_team);
            },
        )
        .map_err(WriteError::ConfigRenderFailed)
    }
}