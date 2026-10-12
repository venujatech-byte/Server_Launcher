use crate::commands::CommandModalState;
use crate::config::{CustomAction, CustomLink, CustomScript, SavedCommand, ServerConfig, SshRemoteHost};
use crate::scripts::ScriptModalState;
use crate::systemd::NewServiceDefinition;
use crate::theme::ThemePalette;
use egui::{Color32, Context, Key, RichText, Rounding, Stroke, Window};
use std::collections::HashMap;

pub struct ServerTemplate {
    pub name: &'static str,
    pub command: &'static str,
    pub stop_command: &'static str,
    pub port: u16,
    pub group: &'static str,
    pub env: &'static [(&'static str, &'static str)],
    pub actions: &'static [(&'static str, &'static str)],
    pub links: &'static [(&'static str, &'static str)],
}

pub const SERVER_TEMPLATES: &[ServerTemplate] = &[
    // --- Cloud Emulators & Local BaaS ---
    ServerTemplate {
        name: "Firebase Emulator Suite",
        command: "firebase emulators:start",
        stop_command: "",
        port: 4000,
        group: "Emulators",
        env: &[],
        actions: &[
            ("Export Data", "firebase emulators:export ./firebase-export"),
            ("Import & Start", "firebase emulators:start --import ./firebase-export"),
        ],
        links: &[
            ("Emulator UI", "http://localhost:4000"),
            ("Auth (9099)", "http://localhost:9099"),
            ("Firestore (8080)", "http://localhost:8080"),
            ("Functions (5001)", "http://localhost:5001"),
            ("Storage (9199)", "http://localhost:9199"),
        ],
    },
    ServerTemplate {
        name: "Supabase (Local CLI)",
        command: "npx supabase start",
        stop_command: "npx supabase stop",
        port: 54323,
        group: "Emulators",
        env: &[],
        actions: &[
            ("Status", "npx supabase status"),
            ("DB Reset", "npx supabase db reset"),
            ("DB Diff", "npx supabase db diff"),
        ],
        links: &[
            ("Studio UI", "http://localhost:54323"),
            ("API Gateway", "http://localhost:54321"),
            ("Inbucket Mail", "http://localhost:54324"),
        ],
    },
    ServerTemplate {
        name: "LocalStack (AWS Emulator)",
        command: "localstack start",
        stop_command: "localstack stop",
        port: 4566,
        group: "Emulators",
        env: &[("SERVICES", "s3,dynamodb,sqs,sns,lambda")],
        actions: &[
            ("Status", "localstack status"),
            ("Health", "curl http://localhost:4566/_localstack/health"),
        ],
        links: &[("Health Check", "http://localhost:4566/_localstack/health")],
    },
    ServerTemplate {
        name: "Azurite (Azure Emulator)",
        command: "npx azurite --location ./azurite-data --silent",
        stop_command: "",
        port: 10000,
        group: "Emulators",
        env: &[],
        actions: &[("Blob Only", "npx azurite-blob --silent")],
        links: &[("Blob Service", "http://localhost:10000")],
    },

    // --- Databases & Stores ---
    ServerTemplate {
        name: "PostgreSQL",
        command: "postgres -D ./data",
        stop_command: "pg_ctl stop -D ./data -m fast",
        port: 5432,
        group: "Database",
        env: &[("PGDATA", "./data")],
        actions: &[
            ("Init DB", "initdb -D ./data"),
            ("Status", "pg_ctl status -D ./data"),
            ("Docker Run", "docker run -d --name local-postgres -p 5432:5432 -e POSTGRES_PASSWORD=postgres postgres:alpine"),
        ],
        links: &[("pgAdmin", "http://localhost:5050")],
    },
    ServerTemplate {
        name: "MySQL",
        command: "mysqld --console",
        stop_command: "mysqladmin -u root shutdown",
        port: 3306,
        group: "Database",
        env: &[],
        actions: &[
            ("Ping", "mysqladmin -u root ping"),
            ("Docker Run", "docker run -d --name local-mysql -p 3306:3306 -e MYSQL_ROOT_PASSWORD=root -e MYSQL_DATABASE=app mysql:latest"),
        ],
        links: &[("phpMyAdmin", "http://localhost:8080")],
    },
    ServerTemplate {
        name: "MariaDB",
        command: "mariadbd --console",
        stop_command: "mariadb-admin -u root shutdown",
        port: 3306,
        group: "Database",
        env: &[],
        actions: &[
            ("Ping", "mariadb-admin -u root ping"),
            ("Docker Run", "docker run -d --name local-mariadb -p 3306:3306 -e MARIADB_ROOT_PASSWORD=root mariadb:latest"),
        ],
        links: &[("phpMyAdmin", "http://localhost:8080")],
    },
    ServerTemplate {
        name: "Redis",
        command: "redis-server",
        stop_command: "redis-cli shutdown",
        port: 6379,
        group: "Database",
        env: &[],
        actions: &[
            ("Ping", "redis-cli ping"),
            ("Monitor", "redis-cli monitor"),
            ("Docker Run", "docker run -d --name local-redis -p 6379:6379 redis:alpine"),
        ],
        links: &[("Redis Insight", "http://localhost:5540")],
    },
    ServerTemplate {
        name: "MongoDB",
        command: "mongod --dbpath ./data/db",
        stop_command: "mongod --shutdown",
        port: 27017,
        group: "Database",
        env: &[],
        actions: &[
            ("Docker Run", "docker run -d --name local-mongo -p 27017:27017 mongo:latest"),
        ],
        links: &[("Mongo Express", "http://localhost:8081")],
    },
    ServerTemplate {
        name: "PocketBase",
        command: "./pocketbase serve --http 0.0.0.0:8090",
        stop_command: "",
        port: 8090,
        group: "Database",
        env: &[],
        actions: &[("Superuser", "./pocketbase superuser create")],
        links: &[
            ("Admin UI", "http://localhost:8090/_/"),
            ("API Docs", "http://localhost:8090/api/"),
        ],
    },
    ServerTemplate {
        name: "MeiliSearch",
        command: "meilisearch --db-path ./meili_data",
        stop_command: "",
        port: 7700,
        group: "Database",
        env: &[("MEILI_ENV", "development")],
        actions: &[("Health", "curl http://localhost:7700/health")],
        links: &[("Health Check", "http://localhost:7700/health")],
    },
    ServerTemplate {
        name: "Elasticsearch",
        command: "./bin/elasticsearch",
        stop_command: "",
        port: 9200,
        group: "Database",
        env: &[],
        actions: &[("Cluster Health", "curl http://localhost:9200/_cluster/health")],
        links: &[
            ("Cluster Info", "http://localhost:9200"),
            ("Kibana", "http://localhost:5601"),
        ],
    },

    // --- Local AI / LLM Tools ---
    ServerTemplate {
        name: "Ollama (Local AI/LLM)",
        command: "ollama serve",
        stop_command: "",
        port: 11434,
        group: "AI/LLM",
        env: &[("OLLAMA_HOST", "0.0.0.0:11434")],
        actions: &[
            ("Pull Llama3", "ollama pull llama3"),
            ("Pull Mistral", "ollama pull mistral"),
            ("List Models", "ollama list"),
        ],
        links: &[
            ("API Tags", "http://localhost:11434/api/tags"),
            ("Open WebUI", "http://localhost:3000"),
        ],
    },
    ServerTemplate {
        name: "LiteLLM Proxy",
        command: "litellm --port 4000",
        stop_command: "",
        port: 4000,
        group: "AI/LLM",
        env: &[],
        actions: &[("Health", "curl http://localhost:4000/health")],
        links: &[("Proxy UI / Docs", "http://localhost:4000")],
    },

    // --- Developer Utilities & Infrastructure ---
    ServerTemplate {
        name: "MinIO (S3 Object Storage)",
        command: "minio server ./data --console-address :9001",
        stop_command: "",
        port: 9001,
        group: "Tools",
        env: &[
            ("MINIO_ROOT_USER", "admin"),
            ("MINIO_ROOT_PASSWORD", "password123"),
        ],
        actions: &[("Docker Run", "docker run -d --name local-minio -p 9000:9000 -p 9001:9001 minio/minio server /data --console-address :9001")],
        links: &[
            ("Console UI", "http://localhost:9001"),
            ("S3 API", "http://localhost:9000"),
        ],
    },
    ServerTemplate {
        name: "Mailpit (SMTP Testing)",
        command: "mailpit",
        stop_command: "",
        port: 8025,
        group: "Tools",
        env: &[
            ("MP_UI_BIND_ADDR", "0.0.0.0:8025"),
            ("MP_SMTP_BIND_ADDR", "0.0.0.0:1025"),
        ],
        actions: &[("Docker Run", "docker run -d --name local-mailpit -p 8025:8025 -p 1025:1025 axllent/mailpit")],
        links: &[("Webmail UI", "http://localhost:8025")],
    },
    ServerTemplate {
        name: "RabbitMQ",
        command: "rabbitmq-server",
        stop_command: "rabbitmqctl stop",
        port: 15672,
        group: "Tools",
        env: &[],
        actions: &[
            ("Status", "rabbitmqctl status"),
            ("Docker Run", "docker run -d --name local-rabbitmq -p 5672:5672 -p 15672:15672 rabbitmq:3-management"),
        ],
        links: &[("Management UI", "http://localhost:15672")],
    },
    ServerTemplate {
        name: "Docker Compose",
        command: "docker compose up",
        stop_command: "docker compose down",
        port: 8080,
        group: "Tools",
        env: &[],
        actions: &[
            ("Build & Up", "docker compose up --build"),
            ("Down", "docker compose down"),
            ("Logs", "docker compose logs -f"),
            ("Status", "docker compose ps"),
        ],
        links: &[("Localhost", "http://localhost:8080")],
    },
    ServerTemplate {
        name: "Caddy Web Server",
        command: "caddy run --config ./Caddyfile",
        stop_command: "caddy stop",
        port: 80,
        group: "Tools",
        env: &[],
        actions: &[("Reload", "caddy reload")],
        links: &[("Localhost", "http://localhost")],
    },

    // --- Frontend Frameworks ---
    ServerTemplate {
        name: "React (Vite)",
        command: "npm run dev",
        stop_command: "",
        port: 5173,
        group: "Frontend",
        env: &[],
        actions: &[("Build", "npm run build"), ("Install", "npm install")],
        links: &[("Localhost", "http://localhost:5173")],
    },
    ServerTemplate {
        name: "Vue (Vite)",
        command: "npm run dev",
        stop_command: "",
        port: 5173,
        group: "Frontend",
        env: &[],
        actions: &[("Build", "npm run build"), ("Install", "npm install")],
        links: &[("Localhost", "http://localhost:5173")],
    },
    ServerTemplate {
        name: "Next.js",
        command: "npm run dev",
        stop_command: "",
        port: 3000,
        group: "Frontend",
        env: &[],
        actions: &[("Build", "npm run build"), ("Install", "npm install")],
        links: &[("Localhost", "http://localhost:3000")],
    },
    ServerTemplate {
        name: "Nuxt",
        command: "npx nuxi dev",
        stop_command: "",
        port: 3000,
        group: "Frontend",
        env: &[],
        actions: &[("Build", "npx nuxi build"), ("Install", "npm install")],
        links: &[("Localhost", "http://localhost:3000")],
    },
    ServerTemplate {
        name: "SvelteKit",
        command: "npm run dev",
        stop_command: "",
        port: 5173,
        group: "Frontend",
        env: &[],
        actions: &[("Build", "npm run build"), ("Check", "npm run check")],
        links: &[("Localhost", "http://localhost:5173")],
    },
    ServerTemplate {
        name: "Angular",
        command: "ng serve",
        stop_command: "",
        port: 4200,
        group: "Frontend",
        env: &[],
        actions: &[("Build", "ng build"), ("Test", "ng test")],
        links: &[("Localhost", "http://localhost:4200")],
    },
    ServerTemplate {
        name: "Astro",
        command: "npm run dev",
        stop_command: "",
        port: 4321,
        group: "Frontend",
        env: &[],
        actions: &[("Build", "npm run build"), ("Check", "npx astro check")],
        links: &[("Localhost", "http://localhost:4321")],
    },
    ServerTemplate {
        name: "Storybook",
        command: "npm run storybook",
        stop_command: "",
        port: 6006,
        group: "Frontend",
        env: &[],
        actions: &[("Build Storybook", "npm run build-storybook")],
        links: &[("Storybook UI", "http://localhost:6006")],
    },

    // --- Backend Frameworks ---
    ServerTemplate {
        name: "FastAPI (Uvicorn)",
        command: "uvicorn main:app --reload --port 8000",
        stop_command: "",
        port: 8000,
        group: "Backend",
        env: &[],
        actions: &[("Install Reqs", "pip install -r requirements.txt")],
        links: &[
            ("Swagger Docs", "http://localhost:8000/docs"),
            ("ReDoc", "http://localhost:8000/redoc"),
        ],
    },
    ServerTemplate {
        name: "Flask",
        command: "flask run --port 5000 --debug",
        stop_command: "",
        port: 5000,
        group: "Backend",
        env: &[("FLASK_ENV", "development"), ("FLASK_DEBUG", "1")],
        actions: &[("Install Reqs", "pip install -r requirements.txt")],
        links: &[("Localhost", "http://localhost:5000")],
    },
    ServerTemplate {
        name: "Django",
        command: "python manage.py runserver 0.0.0.0:8000",
        stop_command: "",
        port: 8000,
        group: "Backend",
        env: &[],
        actions: &[
            ("Migrate", "python manage.py migrate"),
            ("Make Migrations", "python manage.py makemigrations"),
            ("Shell", "python manage.py shell"),
        ],
        links: &[("Admin Panel", "http://localhost:8000/admin")],
    },
    ServerTemplate {
        name: "Node / Express",
        command: "node server.js",
        stop_command: "",
        port: 3000,
        group: "Backend",
        env: &[],
        actions: &[("Install", "npm install")],
        links: &[("Localhost", "http://localhost:3000")],
    },
    ServerTemplate {
        name: "NestJS",
        command: "npm run start:dev",
        stop_command: "",
        port: 3000,
        group: "Backend",
        env: &[],
        actions: &[("Build", "npm run build"), ("Test", "npm run test")],
        links: &[
            ("Localhost", "http://localhost:3000"),
            ("Swagger Docs", "http://localhost:3000/api"),
        ],
    },
    ServerTemplate {
        name: "Spring Boot (Maven)",
        command: "./mvnw spring-boot:run",
        stop_command: "",
        port: 8080,
        group: "Backend",
        env: &[],
        actions: &[("Test", "./mvnw test"), ("Package", "./mvnw clean package")],
        links: &[
            ("Localhost", "http://localhost:8080"),
            ("Actuator Health", "http://localhost:8080/actuator/health"),
        ],
    },
    ServerTemplate {
        name: "Go (Air Live Reload)",
        command: "air",
        stop_command: "",
        port: 8080,
        group: "Backend",
        env: &[],
        actions: &[("Go Run", "go run ."), ("Go Test", "go test ./...")],
        links: &[("Localhost", "http://localhost:8080")],
    },
    ServerTemplate {
        name: "Rust (Cargo Watch)",
        command: "cargo watch -x run",
        stop_command: "",
        port: 8080,
        group: "Backend",
        env: &[("RUST_LOG", "debug")],
        actions: &[("Cargo Check", "cargo check"), ("Cargo Test", "cargo test")],
        links: &[("Localhost", "http://localhost:8080")],
    },
    ServerTemplate {
        name: "Laravel (Artisan)",
        command: "php artisan serve --port 8000",
        stop_command: "",
        port: 8000,
        group: "Backend",
        env: &[],
        actions: &[
            ("Migrate", "php artisan migrate"),
            ("Queue Work", "php artisan queue:work"),
            ("Route List", "php artisan route:list"),
        ],
        links: &[("Localhost", "http://localhost:8000")],
    },
    ServerTemplate {
        name: "Ruby on Rails",
        command: "bin/rails server -p 3000",
        stop_command: "",
        port: 3000,
        group: "Backend",
        env: &[],
        actions: &[
            ("DB Migrate", "bin/rails db:migrate"),
            ("Rails Console", "bin/rails console"),
        ],
        links: &[("Localhost", "http://localhost:3000")],
    },
    ServerTemplate {
        name: "ASP.NET Core (dotnet watch)",
        command: "dotnet watch run",
        stop_command: "",
        port: 5000,
        group: "Backend",
        env: &[("ASPNETCORE_ENVIRONMENT", "Development")],
        actions: &[("Build", "dotnet build"), ("Test", "dotnet test")],
        links: &[
            ("Localhost", "http://localhost:5000"),
            ("Swagger Docs", "http://localhost:5000/swagger"),
        ],
    },
    ServerTemplate {
        name: "Phoenix (Elixir)",
        command: "mix phx.server",
        stop_command: "",
        port: 4000,
        group: "Backend",
        env: &[],
        actions: &[
            ("Ecto Migrate", "mix ecto.migrate"),
            ("IEx Server", "iex -S mix phx.server"),
        ],
        links: &[
            ("Localhost", "http://localhost:4000"),
            ("LiveDashboard", "http://localhost:4000/dashboard"),
        ],
    },
    ServerTemplate {
        name: "Strapi CMS",
        command: "npm run develop",
        stop_command: "",
        port: 1337,
        group: "Backend",
        env: &[],
        actions: &[("Build", "npm run build"), ("Install", "npm install")],
        links: &[
            ("Admin Panel", "http://localhost:1337/admin"),
            ("API", "http://localhost:1337/api"),
        ],
    },

    // --- Mobile Frameworks ---
    ServerTemplate {
        name: "React Native (Metro)",
        command: "npx react-native start",
        stop_command: "",
        port: 8081,
        group: "Mobile",
        env: &[],
        actions: &[
            ("Android", "npx react-native run-android"),
            ("iOS", "npx react-native run-ios"),
            ("Install", "npm install"),
            ("Reset Cache", "npx react-native start --reset-cache"),
        ],
        links: &[
            ("Metro Bundler", "http://localhost:8081"),
            ("Metro Status", "http://localhost:8081/status"),
        ],
    },
    ServerTemplate {
        name: "React Native (Expo)",
        command: "npx expo start",
        stop_command: "",
        port: 8081,
        group: "Mobile",
        env: &[],
        actions: &[
            ("Android", "npx expo run:android"),
            ("iOS", "npx expo run:ios"),
            ("Web", "npx expo start --web"),
            ("Install", "npx expo install"),
        ],
        links: &[("Metro Bundler", "http://localhost:8081")],
    },
];

#[derive(Default)]
pub struct AddEditModalState {
    pub open: bool,
    pub is_edit: bool,
    pub original_key: String,

    pub selected_template_idx: usize,
    pub key: String,
    pub name: String,
    pub group: String,
    pub cwd: String,
    pub command: String,
    pub stop_command: String,
    pub port_str: String,
    pub own_console: bool,
    pub auto_restart: bool,
    pub autostart: bool,
    pub env_text: String,
    pub actions: Vec<(String, String)>,
    pub links: Vec<(String, String)>,
    pub error_msg: Option<String>,

    pub server_type: String,
    pub url: String,
    pub headers: Vec<(String, String)>,

    pub paste_json_open: bool,
    pub paste_json_input: String,
    pub paste_json_msg: Option<(String, bool)>,
}

impl AddEditModalState {
    pub fn open_new(&mut self) {
        self.open = true;
        self.is_edit = false;
        self.original_key.clear();
        self.selected_template_idx = 0;
        self.key.clear();
        self.name.clear();
        self.group = "General".to_string();
        self.cwd = ".".to_string();
        self.command.clear();
        self.stop_command.clear();
        self.port_str = "0".to_string();
        self.own_console = false;
        self.auto_restart = false;
        self.autostart = false;
        self.env_text.clear();
        self.actions.clear();
        self.links.clear();
        self.error_msg = None;
        self.server_type = "cli".to_string();
        self.url.clear();
        self.headers.clear();
        self.paste_json_open = false;
        self.paste_json_input.clear();
        self.paste_json_msg = None;
    }

    #[allow(dead_code)]
    pub fn open_import(&mut self, name: &str, port: u16, command: &str, cwd: &str) {
        self.open_new();
        self.name = name.to_string();
        self.key = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .trim_matches('_')
            .to_string();
        if self.key.is_empty() {
            self.key = format!("port_{}", port);
        }
        self.port_str = port.to_string();
        self.command = command.to_string();
        self.cwd = if cwd.is_empty() { ".".to_string() } else { cwd.to_string() };
        self.group = "Imported".to_string();
        if port > 0 {
            self.links.push(("Localhost".to_string(), format!("http://localhost:{}", port)));
        }
    }

    pub fn open_edit(&mut self, cfg: &ServerConfig) {
        self.open = true;
        self.is_edit = true;
        self.original_key = cfg.key.clone();
        self.selected_template_idx = 0;
        self.key = cfg.key.clone();
        self.name = cfg.name.clone();
        self.group = cfg.group.clone();
        self.cwd = cfg.cwd.clone();
        self.command = cfg.command.clone();
        self.stop_command = cfg.stop_command.clone();
        self.port_str = cfg.port.to_string();
        self.own_console = cfg.own_console;
        self.auto_restart = cfg.auto_restart;
        self.autostart = cfg.autostart;

        let mut env_lines = Vec::new();
        for (k, v) in &cfg.env {
            env_lines.push(format!("{}={}", k, v));
        }
        self.env_text = env_lines.join("\n");

        self.actions = cfg
            .actions
            .iter()
            .map(|a| (a.label.clone(), a.command.clone()))
            .collect();
        self.links = cfg
            .links
            .iter()
            .map(|l| (l.label.clone(), l.url.clone()))
            .collect();
        self.error_msg = None;

        self.server_type = cfg.server_type.clone().unwrap_or_else(|| "cli".to_string());
        self.url = cfg.url.clone().unwrap_or_default();
        let mut headers: Vec<(String, String)> = cfg
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        headers.sort_by(|a, b| a.0.cmp(&b.0));
        self.headers = headers;
        self.paste_json_open = false;
        self.paste_json_input.clear();
        self.paste_json_msg = None;
    }

    pub fn apply_template(&mut self, idx: usize) {
        if idx == 0 || idx > SERVER_TEMPLATES.len() {
            return;
        }
        let tpl = &SERVER_TEMPLATES[idx - 1];
        if self.name.trim().is_empty() {
            self.name = tpl.name.to_string();
        }
        if self.key.trim().is_empty() {
            self.key = tpl
                .name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect::<String>()
                .trim_matches('_')
                .to_string();
        }
        self.command = tpl.command.to_string();
        self.stop_command = tpl.stop_command.to_string();
        self.port_str = tpl.port.to_string();
        self.group = tpl.group.to_string();

        let env_lines: Vec<String> = tpl
            .env
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        self.env_text = env_lines.join("\n");

        self.actions = tpl
            .actions
            .iter()
            .map(|(l, c)| (l.to_string(), c.to_string()))
            .collect();
        self.links = tpl
            .links
            .iter()
            .map(|(l, u)| (l.to_string(), u.to_string()))
            .collect();
    }

    #[allow(dead_code)]
    pub fn apply_json_snippet(&mut self, input: &str) -> Result<String, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err("Nothing to parse - paste some JSON first.".to_string());
        }
        if self.cwd.trim().is_empty() {
            self.cwd = ".".to_string();
        }
        let value: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|e| format!("Invalid JSON: {}", e))?;
        let obj = value
            .as_object()
            .ok_or_else(|| "Expected a JSON object.".to_string())?;

        let (entry_key, entry) = if let Some(servers) = obj
            .get("servers")
            .or_else(|| obj.get("mcpServers"))
            .and_then(|v| v.as_object())
        {
            servers
                .iter()
                .next()
                .map(|(k, v)| (k.clone(), v))
                .ok_or_else(|| "The servers object is empty.".to_string())?
        } else if obj.contains_key("url") || obj.contains_key("command") || obj.contains_key("type") {
            let key = obj
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("custom")
                .to_string();
            (key, &value)
        } else {
            obj.iter()
                .find(|(_, v)| v.is_object())
                .map(|(k, v)| (k.clone(), v))
                .ok_or_else(|| "Could not find a server definition in the JSON.".to_string())?
        };

        let entry_obj = entry
            .as_object()
            .ok_or_else(|| "Server definition must be a JSON object.".to_string())?;

        let declared_type = entry_obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        self.server_type = if declared_type.contains("http") || entry_obj.contains_key("url") {
            "http".to_string()
        } else {
            "cli".to_string()
        };

        self.key = entry_key
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .trim_matches('_')
            .to_string();
        if self.key.is_empty() {
            self.key = "custom_mcp".to_string();
        }

        self.name = entry_obj
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                entry_key
                    .split(['_', '-', '.'])
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        let mut chars = s.chars();
                        match chars.next() {
                            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                            None => String::new(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            });

        self.url = entry_obj
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        self.headers.clear();
        if let Some(h) = entry_obj.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in h {
                let val = v
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| v.to_string());
                self.headers.push((k.clone(), val));
            }
        }

        if let Some(env) = entry_obj.get("env").and_then(|v| v.as_object()) {
            let lines: Vec<String> = env
                .iter()
                .map(|(k, v)| format!("{}={}", k, v.as_str().unwrap_or("")))
                .collect();
            if !lines.is_empty() {
                self.env_text = lines.join("\n");
            }
        }

        if self.server_type == "http" {
            self.group = "MCP".to_string();
            let mut h_args = String::new();
            for (hk, hv) in &self.headers {
                if !hk.trim().is_empty() {
                    h_args.push_str(&format!(" -H \"{}: {}\"", hk.trim(), hv.trim()));
                }
            }
            self.command = format!("curl -i{} {}", h_args, self.url.trim());
            if !self.actions.iter().any(|(l, _)| l == "Test Ping") {
                self.actions
                    .push(("Test Ping".to_string(), self.command.clone()));
            }
            if !self.url.trim().is_empty()
                && !self.links.iter().any(|(l, _)| l == "MCP Endpoint")
            {
                self.links
                    .push(("MCP Endpoint".to_string(), self.url.trim().to_string()));
            }
        } else if let Some(cmd) = entry_obj.get("command").and_then(|v| v.as_str()) {
            let mut full = cmd.to_string();
            if let Some(args) = entry_obj.get("args").and_then(|v| v.as_array()) {
                for a in args.iter().filter_map(|a| a.as_str()) {
                    full.push(' ');
                    full.push_str(a);
                }
            }
            self.command = full;
        }

        self.paste_json_msg = None;
        Ok(format!(
            "Autofilled '{}' from pasted JSON. Review the fields below and save.",
            entry_key
        ))
    }

    pub fn to_server_config(&self) -> Result<ServerConfig, String> {
        let name = self.name.trim().to_string();
        let cwd = self.cwd.trim().to_string();
        let mut command = self.command.trim().to_string();

        if name.is_empty() {
            return Err("Server name is required.".to_string());
        }
        if cwd.is_empty() {
            return Err("Working directory is required.".to_string());
        }
        if self.server_type == "http" && self.url.trim().is_empty() {
            return Err("Endpoint URL is required for HTTP / MCP servers.".to_string());
        }
        if command.is_empty() {
            if self.server_type == "http" {
                let mut h_args = String::new();
                for (hk, hv) in &self.headers {
                    if !hk.trim().is_empty() {
                        h_args.push_str(&format!(" -H \"{}: {}\"", hk.trim(), hv.trim()));
                    }
                }
                command = format!("curl -i{} {}", h_args, self.url.trim());
            } else {
                return Err("Start command is required.".to_string());
            }
        }

        let key = if self.key.trim().is_empty() {
            let derived = name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect::<String>()
                .trim_matches('_')
                .to_string();
            if derived.is_empty() {
                format!("custom_{}", chrono::Local::now().timestamp())
            } else {
                derived
            }
        } else {
            self.key.trim().to_string()
        };

        let port = self.port_str.trim().parse::<u16>().unwrap_or(0);
        let group = if self.group.trim().is_empty() {
            "General".to_string()
        } else {
            self.group.trim().to_string()
        };

        // Parse environment variables
        let mut env = HashMap::new();
        for line in self.env_text.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') {
                if let Some((k, v)) = line.split_once('=') {
                    env.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }

        // Actions
        let actions: Vec<CustomAction> = self
            .actions
            .iter()
            .filter(|(l, c)| !l.trim().is_empty() && !c.trim().is_empty())
            .map(|(l, c)| CustomAction {
                label: l.trim().to_string(),
                command: c.trim().to_string(),
            })
            .collect();

        // Links
        let links: Vec<CustomLink> = self
            .links
            .iter()
            .filter(|(l, u)| !l.trim().is_empty() && !u.trim().is_empty())
            .map(|(l, u)| CustomLink {
                label: l.trim().to_string(),
                url: u.trim().to_string(),
            })
            .collect();

        Ok(ServerConfig {
            key,
            name,
            cwd,
            command,
            stop_command: self.stop_command.trim().to_string(),
            port,
            group,
            own_console: self.own_console,
            auto_restart: self.auto_restart,
            autostart: self.autostart,
            env,
            actions,
            links,
            server_type: if self.server_type == "http" {
                Some("http".to_string())
            } else {
                None
            },
            url: if self.server_type == "http" && !self.url.trim().is_empty() {
                Some(self.url.trim().to_string())
            } else {
                None
            },
            headers: self
                .headers
                .iter()
                .filter(|(k, _)| !k.trim().is_empty())
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .collect::<HashMap<String, String>>(),
        })
    }
}

#[allow(dead_code)]
pub enum ModalAction {
    SaveServer(ServerConfig, Option<String>),
    DeleteServer(String),
    SaveSshHost(crate::config::SshRemoteHost, Option<String>),
    DeleteSshHost(String),
    ExecutePaletteAction(PaletteAction),
    None,
}

#[derive(Clone, Debug)]
pub enum PaletteAction {
    StartServer(String),
    StopServer(String),
    RestartServer(String),
    SelectTab(String),
    RunAction { key: String, command: String },
    OpenLink(String),
    StartGroup(String),
    StopGroup(String),
    StartAll,
    StopAll,
    SetTheme(String),
    ToggleAppAutostart,
    ToggleServerAutostart(String),
}

#[derive(Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected_idx: usize,
}

pub fn render_modal_titlebar(
    ui: &mut egui::Ui,
    title: &str,
    is_open: &mut bool,
) {
    let titlebar_height = 28.0;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), titlebar_height),
        egui::Sense::drag(),
    );

    // Draggable window support using layer offset
    if resp.dragged() {
        let delta = resp.drag_delta();
        let layer_id = ui.layer_id();
        let offset_id = layer_id.id.with("modal_drag_offset");
        let current_offset = ui.data(|d| d.get_temp::<egui::Vec2>(offset_id)).unwrap_or_default();
        let new_offset = current_offset + delta;
        ui.data_mut(|d| d.insert_temp(offset_id, new_offset));
        ui.ctx().request_repaint();
    }

    let painter = ui.painter();

    // 1. Left side: glowing teal/green dot + "SERVER LAUNCHER"
    let dot_center = egui::pos2(rect.min.x + 14.0, rect.center().y);
    painter.circle_filled(dot_center, 3.5, Color32::from_rgb(0, 210, 160));
    painter.text(
        egui::pos2(rect.min.x + 24.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "SERVER LAUNCHER",
        egui::FontId::monospace(10.0),
        Color32::from_rgb(113, 118, 134),
    );

    // 2. Center: Modal title
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(13.0),
        Color32::from_rgb(220, 224, 235),
    );

    // 3. Right side: [] and X buttons
    let btn_w = 28.0;
    let btn_h = 22.0;
    let close_rect = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - btn_w - 6.0, rect.center().y - btn_h / 2.0),
        egui::vec2(btn_w, btn_h),
    );
    let square_rect = egui::Rect::from_min_size(
        egui::pos2(close_rect.min.x - btn_w - 2.0, rect.center().y - btn_h / 2.0),
        egui::vec2(btn_w, btn_h),
    );

    let square_resp = ui.interact(square_rect, ui.id().with("modal_square_btn"), egui::Sense::click());
    if square_resp.hovered() {
        painter.rect_filled(square_rect, Rounding::same(4.0), Color32::from_rgba_unmultiplied(255, 255, 255, 18));
    }
    let sq_center = square_rect.center();
    let sq_icon_rect = egui::Rect::from_center_size(sq_center, egui::vec2(8.5, 8.5));
    painter.rect_stroke(
        sq_icon_rect,
        Rounding::ZERO,
        Stroke::new(1.1_f32, if square_resp.hovered() { Color32::from_rgb(220, 224, 235) } else { Color32::from_rgb(140, 146, 162) }),
    );

    let close_resp = ui.interact(close_rect, ui.id().with("modal_close_btn"), egui::Sense::click());
    if close_resp.hovered() {
        painter.rect_filled(close_rect, Rounding::same(4.0), Color32::from_rgb(220, 53, 69));
    }
    let cl_center = close_rect.center();
    let d = 4.0;
    let cl_stroke = Stroke::new(
        1.2_f32,
        if close_resp.hovered() { Color32::WHITE } else { Color32::from_rgb(140, 146, 162) },
    );
    painter.line_segment([egui::pos2(cl_center.x - d, cl_center.y - d), egui::pos2(cl_center.x + d, cl_center.y + d)], cl_stroke);
    painter.line_segment([egui::pos2(cl_center.x + d, cl_center.y - d), egui::pos2(cl_center.x - d, cl_center.y + d)], cl_stroke);

    if close_resp.clicked() {
        *is_open = false;
    }
}

pub fn render_add_edit_modal(ctx: &Context, state: &mut AddEditModalState, palette: &ThemePalette) -> ModalAction {
    if !state.open {
        return ModalAction::None;
    }

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.open = false;
        return ModalAction::None;
    }

    let mut action = ModalAction::None;
    let window_title = if state.is_edit {
        "Edit Server"
    } else {
        "Add Server"
    };

    let screen = ctx.screen_rect();
    let modal_w = 560.0_f32.min(screen.width() - 40.0).max(380.0);
    let max_h = (screen.height() - 40.0).max(400.0);

    let mut is_open = state.open;
    let window_id = egui::Id::new("add_edit_server_modal_window");
    let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();
    Window::new(window_title)
        .id(window_id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .current_pos(screen.center() + drag_offset)
        .pivot(egui::Align2::CENTER_CENTER)
        .min_width(modal_w)
        .max_width(modal_w)
        .default_height(640.0)
        .max_height(max_h)
        .frame(
            egui::Frame::none()
                .fill(Color32::from_rgb(17, 19, 27))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                .rounding(Rounding::same(16.0))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 8.0),
                    blur: 24.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(160),
                })
                .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
        )
        .show(ctx, |ui| {
            render_modal_titlebar(ui, window_title, &mut is_open);
            ui.add_space(8.0);
            ui.set_max_width(ui.available_width());

        // ── Subtitle + servers.json badge ──
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new("Saved to").size(11.0).color(palette.text_muted));
            egui::Frame::none()
                .fill(Color32::from_rgb(22, 25, 35))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 58)))
                .rounding(Rounding::same(3.0))
                .inner_margin(egui::Margin::symmetric(5.0, 1.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("servers.json")
                            .size(10.5)
                            .monospace()
                            .color(Color32::from_rgb(180, 185, 200)),
                    );
                });
            ui.label(RichText::new("— no code changes needed.").size(11.0).color(palette.text_muted));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::Frame::none()
                    .fill(Color32::from_rgb(14, 15, 22))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 50)))
                    .rounding(Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("servers.json")
                                .size(10.5)
                                .monospace()
                                .color(Color32::from_rgb(110, 118, 138)),
                        );
                    });
            });
        });

        ui.add_space(10.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_width(ui.available_width())
                .max_height(ui.available_height() - 55.0)
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width());

                    // Template Picker (only for new servers)
                    if !state.is_edit {
                        egui::Frame::none()
                            .fill(Color32::from_rgb(19, 21, 30))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 52)))
                            .rounding(Rounding::same(10.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("⚡").size(13.0).color(Color32::from_rgb(196, 192, 255)));
                                    ui.label(
                                        RichText::new("Pick a Template (Fast Setup)")
                                            .size(12.5)
                                            .strong()
                                            .color(Color32::from_rgb(196, 192, 255)),
                                    );
                                });
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new(
                                        "Auto-populates command, port, actions, and links for popular stacks.",
                                    )
                                    .size(10.5)
                                    .color(palette.text_muted),
                                );
                                ui.add_space(8.0);

                                ui.horizontal(|ui| {
                                    let mut template_names = vec!["-- Select a template --"];
                                    for t in SERVER_TEMPLATES {
                                        template_names.push(t.name);
                                    }

                                    let cur_name = template_names[state.selected_template_idx];
                                    let combo_w = (ui.available_width() - 130.0).max(180.0);
                                    egui::ComboBox::from_id_salt("template_select")
                                        .selected_text(cur_name)
                                        .width(combo_w)
                                        .show_ui(ui, |ui| {
                                            for (i, name) in template_names.iter().enumerate() {
                                                ui.selectable_value(
                                                    &mut state.selected_template_idx,
                                                    i,
                                                    *name,
                                                );
                                            }
                                        });

                                    let apply_btn = egui::Button::new(
                                        RichText::new("Apply Template")
                                            .size(11.5)
                                            .strong()
                                            .color(Color32::from_rgb(24, 25, 34)),
                                    )
                                    .fill(Color32::from_rgb(196, 192, 255))
                                    .rounding(Rounding::same(6.0))
                                    .min_size(egui::vec2(110.0, 26.0));

                                    if ui.add(apply_btn).clicked() {
                                        state.apply_template(state.selected_template_idx);
                                    }
                                });
                            });

                        ui.add_space(14.0);
                    }

                    // Server Name *
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.label(RichText::new("Server name").size(12.0).strong().color(palette.text_primary));
                        ui.label(RichText::new("*").size(12.0).strong().color(palette.danger));
                    });
                    ui.add(
                        egui::TextEdit::singleline(&mut state.name)
                            .hint_text("e.g. Frontend App")
                            .clip_text(true)
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(8.0);

                    // Unique Key (optional override)
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Unique Key (optional override)")
                                .size(11.0)
                                .color(palette.text_muted),
                        );
                        let derived = if !state.key.trim().is_empty() {
                            state.key.trim().to_string()
                        } else {
                            state.name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect::<String>().trim_matches('_').to_string()
                        };
                        if !derived.is_empty() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!("ID: {}", derived))
                                        .size(10.5)
                                        .monospace()
                                        .color(Color32::from_rgb(110, 118, 138)),
                                );
                            });
                        }
                    });
                    ui.add_enabled(
                        !state.is_edit,
                        egui::TextEdit::singleline(&mut state.key)
                            .hint_text("auto-generated if empty")
                            .clip_text(true)
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(8.0);

                    // Server Group
                    ui.label(
                        RichText::new("Server group (Frontend / Backend / DB)")
                            .size(12.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.label(
                        RichText::new("Tag servers for group start/stop. Pick preset or type custom:")
                            .size(10.5)
                            .color(palette.text_muted),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut state.group)
                            .clip_text(true)
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        for preset in &["Frontend", "Backend", "DB", "General"] {
                            let is_active = state.group == *preset;
                            let btn = egui::Button::new(
                                RichText::new(*preset)
                                    .size(10.5)
                                    .color(if is_active { Color32::from_rgb(196, 192, 255) } else { palette.text_secondary }),
                            )
                            .fill(if is_active { Color32::from_rgb(34, 38, 56) } else { Color32::from_rgb(22, 24, 34) })
                            .stroke(if is_active { Stroke::new(1.0_f32, Color32::from_rgb(196, 192, 255)) } else { Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 52)) })
                            .rounding(Rounding::same(6.0));
                            if ui.add(btn).clicked() {
                                state.group = preset.to_string();
                            }
                        }
                    });
                    ui.add_space(8.0);

                    // Working Directory *
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.label(RichText::new("Working directory (path)").size(12.0).strong().color(palette.text_primary));
                        ui.label(RichText::new("*").size(12.0).strong().color(palette.danger));
                    });
                    ui.horizontal(|ui| {
                        let browse_w = 75.0;
                        let input_w = (ui.available_width() - browse_w - ui.spacing().item_spacing.x).max(80.0);
                        ui.add_sized(
                            [input_w, 20.0],
                            egui::TextEdit::singleline(&mut state.cwd)
                                .hint_text("e.g. /path/to/project or .")
                                .clip_text(true),
                        );
                        let browse_btn = egui::Button::new(
                            RichText::new("Browse").size(11.5).color(palette.text_secondary),
                        )
                        .fill(Color32::from_rgb(26, 29, 40))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 58)))
                        .rounding(Rounding::same(6.0));
                        if ui.add(browse_btn).clicked() {
                            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                state.cwd = folder.to_string_lossy().to_string();
                            }
                        }
                    });
                    ui.add_space(8.0);

                    // Start Command *
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.label(RichText::new("Start command").size(12.0).strong().color(palette.text_primary));
                        ui.label(RichText::new("*").size(12.0).strong().color(palette.danger));
                    });
                    ui.label(
                        RichText::new("e.g.  npm run dev   |   python app.py   |   node server.js")
                            .size(10.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                    );
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("$").size(12.0).monospace().color(Color32::from_rgb(110, 118, 138)));
                        ui.add(
                            egui::TextEdit::singleline(&mut state.command)
                                .hint_text("e.g. npm run dev")
                                .clip_text(true)
                                .desired_width(ui.available_width()),
                        );
                    });
                    ui.add_space(8.0);

                    // Stop Command (optional)
                    ui.label(
                        RichText::new("Stop command (optional - runs before stopping)")
                            .size(12.0)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("e.g.  pg_ctl stop   - leave empty for plain kill")
                            .size(10.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut state.stop_command)
                            .clip_text(true)
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(8.0);

                    // Port
                    ui.label(
                        RichText::new("Port (0 = none, used to detect running state)")
                            .size(12.0)
                            .strong(),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut state.port_str)
                            .clip_text(true)
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(8.0);

                    // Own console checkbox
                    ui.checkbox(
                        &mut state.own_console,
                        RichText::new("Run in own console window (external terminal)")
                            .size(12.0)
                            .color(Color32::from_rgb(220, 225, 235)),
                    );
                    ui.label(
                        RichText::new("Opens in a native terminal emulator (gnome-terminal, konsole, cmd.exe, etc.)")
                            .size(10.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                    );
                    ui.add_space(8.0);

                    // Auto-restart on crash checkbox
                    ui.checkbox(
                        &mut state.auto_restart,
                        RichText::new("Auto-restart on crash")
                            .size(12.0)
                            .color(palette.text_primary),
                    );
                    ui.label(
                        RichText::new("Automatically restarts this server if the process exits unexpectedly or crashes")
                            .size(10.5)
                            .color(palette.text_muted),
                    );
                    ui.add_space(8.0);

                    // Run at startup checkbox
                    ui.checkbox(
                        &mut state.autostart,
                        RichText::new("🚀 Run server at startup")
                            .size(12.0)
                            .color(palette.text_primary),
                    );
                    ui.label(
                        RichText::new("Automatically launches this server whenever Server Launcher opens")
                            .size(10.5)
                            .color(palette.text_muted),
                    );
                    ui.add_space(8.0);

                    // Environment Variables
                    ui.label(
                        RichText::new("Environment variables (KEY=VALUE, one per line)")
                            .size(12.0)
                            .strong(),
                    );
                    ui.add(
                        egui::TextEdit::multiline(&mut state.env_text)
                            .clip_text(true)
                            .desired_rows(3)
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(10.0);

                    // Extra Action Buttons
                    ui.label(
                        RichText::new("Extra action buttons (sub-commands)")
                            .size(12.0)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Run inside working directory. e.g. label: 'Build', command: 'npm run build'")
                            .size(10.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                    );
                    ui.add_space(2.0);

                    let mut remove_action_idx = None;
                    for (i, (lbl, cmd)) in state.actions.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("label:").size(10.5).color(Color32::from_rgb(123, 131, 148)));
                            ui.add(egui::TextEdit::singleline(lbl).clip_text(true).desired_width(85.0));
                            ui.label(RichText::new("cmd:").size(10.5).color(Color32::from_rgb(123, 131, 148)));
                            let cmd_w = (ui.available_width() - 36.0).max(60.0);
                            ui.add_sized(
                                [cmd_w, 20.0],
                                egui::TextEdit::singleline(cmd).clip_text(true),
                            );
                            if ui
                                .button(RichText::new("✕").size(11.0).color(Color32::from_rgb(230, 80, 80)))
                                .clicked()
                            {
                                remove_action_idx = Some(i);
                            }
                        });
                        ui.add_space(2.0);
                    }
                    if let Some(i) = remove_action_idx {
                        state.actions.remove(i);
                    }

                    if ui
                        .button(
                            RichText::new("+ Add action button")
                                .size(11.0)
                                .color(Color32::from_rgb(108, 92, 231)),
                        )
                        .clicked()
                    {
                        state.actions.push((String::new(), String::new()));
                    }

                    ui.add_space(10.0);

                    // Open Links
                    ui.label(RichText::new("Open links (label : url)").size(12.0).strong());
                    ui.label(
                        RichText::new("e.g. label: 'Localhost', url: 'http://localhost:3000'")
                            .size(10.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                    );
                    ui.add_space(2.0);

                    let mut remove_link_idx = None;
                    for (i, (lbl, url)) in state.links.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("label:").size(10.5).color(Color32::from_rgb(123, 131, 148)));
                            ui.add(egui::TextEdit::singleline(lbl).clip_text(true).desired_width(85.0));
                            ui.label(RichText::new("url:").size(10.5).color(Color32::from_rgb(123, 131, 148)));
                            let url_w = (ui.available_width() - 36.0).max(60.0);
                            ui.add_sized(
                                [url_w, 20.0],
                                egui::TextEdit::singleline(url).clip_text(true),
                            );
                            if ui
                                .button(RichText::new("✕").size(11.0).color(Color32::from_rgb(230, 80, 80)))
                                .clicked()
                            {
                                remove_link_idx = Some(i);
                            }
                        });
                        ui.add_space(2.0);
                    }
                    if let Some(i) = remove_link_idx {
                        state.links.remove(i);
                    }

                    if ui
                        .button(
                            RichText::new("+ Add link")
                                .size(11.0)
                                .color(palette.accent),
                        )
                        .clicked()
                    {
                        state.links.push((String::new(), String::new()));
                    }

                    ui.add_space(12.0);
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            // Bottom Buttons: Status & Cancel / Save
            ui.horizontal(|ui| {
                if let Some(err) = &state.error_msg {
                    ui.label(RichText::new("●").size(9.0).color(palette.danger));
                    ui.label(RichText::new(err).size(11.0).color(palette.danger));
                } else {
                    ui.label(RichText::new("●").size(9.0).color(Color32::from_rgb(0, 210, 160)));
                    ui.label(
                        RichText::new("Ready to register instance")
                            .size(11.0)
                            .monospace()
                            .color(Color32::from_rgb(120, 126, 142)),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_save = egui::Button::new(
                        RichText::new("Save")
                            .size(12.0)
                            .strong()
                            .color(Color32::from_rgb(24, 25, 34)),
                    )
                    .fill(Color32::from_rgb(196, 192, 255))
                    .rounding(Rounding::same(8.0))
                    .min_size(egui::vec2(85.0, 28.0));

                    if ui.add(btn_save).clicked() {
                        match state.to_server_config() {
                            Ok(cfg) => {
                                let old_key = if state.is_edit {
                                    Some(state.original_key.clone())
                                } else {
                                    None
                                };
                                action = ModalAction::SaveServer(cfg, old_key);
                                state.open = false;
                                is_open = false;
                            }
                            Err(err) => {
                                state.error_msg = Some(err);
                            }
                        }
                    }

                    let btn_cancel = egui::Button::new(
                        RichText::new("Cancel")
                            .size(12.0)
                            .color(palette.text_secondary),
                    )
                    .fill(Color32::from_rgb(26, 29, 40))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 58)))
                    .rounding(Rounding::same(6.0))
                    .min_size(egui::vec2(80.0, 28.0));

                    if ui.add(btn_cancel).clicked() {
                        state.open = false;
                        is_open = false;
                    }
                });
            });
        });

    if !is_open {
        state.open = false;
    }
    action
}

pub fn render_command_palette(
    ctx: &Context,
    state: &mut CommandPaletteState,
    items: &[(String, PaletteAction)],
    palette: &ThemePalette,
) -> ModalAction {
    if !state.open {
        return ModalAction::None;
    }

    let mut action = ModalAction::None;

    // Filter items
    let q = state.query.to_lowercase();
    let tokens: Vec<&str> = q.split_whitespace().collect();
    let filtered: Vec<_> = items
        .iter()
        .filter(|(label, _)| {
            let l_lower = label.to_lowercase();
            tokens.iter().all(|tok| l_lower.contains(tok))
        })
        .collect();

    // Key navigation
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.open = false;
        return ModalAction::None;
    }

    if ctx.input(|i| i.key_pressed(Key::ArrowDown)) && !filtered.is_empty() {
        state.selected_idx = (state.selected_idx + 1) % filtered.len();
    }
    if ctx.input(|i| i.key_pressed(Key::ArrowUp)) && !filtered.is_empty() {
        state.selected_idx = if state.selected_idx == 0 {
            filtered.len() - 1
        } else {
            state.selected_idx - 1
        };
    }

    if ctx.input(|i| i.key_pressed(Key::Enter)) && !filtered.is_empty() {
        let idx = state.selected_idx.min(filtered.len() - 1);
        action = ModalAction::ExecutePaletteAction(filtered[idx].1.clone());
        state.open = false;
        return action;
    }

    let screen = ctx.screen_rect();
    let modal_w = 540.0_f32.min(screen.width() - 40.0).max(360.0);
    let max_h = (screen.height() - 100.0).max(300.0);

    let mut is_open = state.open;
    Window::new("⚡ Quick Launch (Ctrl+P)")
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .min_width(modal_w)
        .max_width(modal_w)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 70.0])
        .default_height(400.0)
        .max_height(max_h)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("⚡ Quick Launch")
                        .size(14.0)
                        .strong()
                        .color(palette.text_primary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(RichText::new("✕").size(12.0).color(palette.text_secondary))
                        .clicked()
                    {
                        state.open = false;
                    }
                    ui.label(
                        RichText::new("Esc to close")
                            .size(10.5)
                            .color(palette.text_muted),
                    );
                });
            });

            ui.add_space(8.0);

            let res = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Search servers, actions, groups, or links…")
                    .desired_width(f32::INFINITY),
            );
            res.request_focus();

            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(280.0)
                .show(ui, |ui| {
                    if filtered.is_empty() {
                        ui.add_space(10.0);
                        ui.label(RichText::new("No matching actions").weak());
                    } else {
                        for (i, (label, act)) in filtered.iter().enumerate() {
                            let selected = i == state.selected_idx;
                            let text = if selected {
                                RichText::new(format!("❯  {}", label))
                                    .color(palette.accent_light)
                                    .strong()
                            } else {
                                RichText::new(format!("   {}", label))
                                    .color(palette.text_secondary)
                            };

                            let resp = ui.selectable_label(selected, text);
                            if resp.clicked() {
                                action = ModalAction::ExecutePaletteAction((*act).clone());
                                state.open = false;
                            }
                        }
                    }
                });

            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Type to filter • ↑/↓ navigate • Enter select")
                        .size(10.0)
                        .color(palette.text_muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(RichText::new("Close").size(11.0).color(palette.text_secondary))
                        .clicked()
                    {
                        state.open = false;
                    }
                });
            });
        });

    if !is_open {
        state.open = false;
    }
    action
}

#[derive(Clone, Default)]
pub struct SshHostModalState {
    pub open: bool,
    pub is_edit: bool,
    pub editing_id: Option<String>,
    pub name: String,
    pub host: String,
    pub port_str: String,
    pub user: String,
    pub key_path: String,
    pub password: String,
    pub show_password: bool,
    pub remote_cwd: String,
    pub test_result: Option<Result<String, String>>,
    pub is_testing: bool,
    pub use_agent: bool,
    pub strict_host_key: bool,
}

impl SshHostModalState {
    pub fn open_add(&mut self) {
        self.open = true;
        self.is_edit = false;
        self.editing_id = None;
        self.name = String::new();
        self.host = String::new();
        self.port_str = "22".to_string();
        self.user = "ubuntu".to_string();
        self.key_path = String::new();
        self.password = String::new();
        self.show_password = false;
        self.remote_cwd = String::new();
        self.test_result = None;
        self.is_testing = false;
        self.use_agent = true;
        self.strict_host_key = false;
    }

    pub fn open_edit(&mut self, h: &crate::config::SshRemoteHost) {
        self.open = true;
        self.is_edit = true;
        self.editing_id = Some(h.id.clone());
        self.name = h.name.clone();
        self.host = h.host.clone();
        self.port_str = h.port.to_string();
        self.user = h.user.clone();
        self.key_path = h.key_path.clone();
        self.password = h.password.clone();
        self.show_password = false;
        self.remote_cwd = h.remote_cwd.clone();
        self.test_result = None;
        self.is_testing = false;
        self.use_agent = true;
        self.strict_host_key = false;
    }
}

pub fn render_ssh_host_modal(ctx: &Context, state: &mut SshHostModalState, palette: &ThemePalette) -> ModalAction {
    if !state.open {
        return ModalAction::None;
    }

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.open = false;
        return ModalAction::None;
    }

    let mut action = ModalAction::None;
    let title = if state.is_edit { "Edit SSH Remote PC" } else { "Add SSH Remote PC" };

    let screen = ctx.screen_rect();
    let modal_w = 540.0_f32.min(screen.width() - 40.0).max(380.0);
    let max_h = (screen.height() - 40.0).max(420.0);

    let mut is_open = state.open;
    let window_id = egui::Id::new("add_edit_ssh_host_modal_window");
    let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();
    Window::new(title)
        .id(window_id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .current_pos(screen.center() + drag_offset)
        .pivot(egui::Align2::CENTER_CENTER)
        .min_width(modal_w)
        .max_width(modal_w)
        .default_height(620.0)
        .max_height(max_h)
        .frame(
            egui::Frame::none()
                .fill(Color32::from_rgb(17, 19, 27))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                .rounding(Rounding::same(16.0))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 8.0),
                    blur: 24.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(160),
                })
                .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
        )
        .show(ctx, |ui| {
            render_modal_titlebar(ui, title, &mut is_open);
            ui.add_space(8.0);
            ui.set_max_width(ui.available_width());

        // ── Subtitle row: Saved to remote_hosts.json ──
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new("Saved to").size(11.0).color(palette.text_muted));
            egui::Frame::none()
                .fill(Color32::from_rgb(22, 25, 35))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 58)))
                .rounding(Rounding::same(3.0))
                .inner_margin(egui::Margin::symmetric(5.0, 1.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("remote_hosts.json").size(10.5).monospace().color(Color32::from_rgb(180, 185, 200)));
                });
            ui.label(RichText::new("— encrypted vault credentials.").size(11.0).color(palette.text_muted));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::Frame::none()
                    .fill(Color32::from_rgb(19, 21, 30))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 52)))
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(RichText::new("●").size(8.0).color(Color32::from_rgb(196, 192, 255)));
                            ui.label(RichText::new("SSH v2 (OpenSSH)").size(10.5).monospace().color(Color32::from_rgb(180, 185, 200)));
                        });
                    });
            });
        });

            ui.add_space(12.0);

            // ── Card: Automatic SSH Config Import Banner ──
            egui::Frame::none()
                .fill(Color32::from_rgb(19, 21, 30))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 52)))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        egui::Frame::none()
                            .fill(Color32::from_rgb(26, 29, 42))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::same(6.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new("⚡").size(13.0).color(Color32::from_rgb(196, 192, 255)));
                            });
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Automatic SSH config import available").size(12.0).strong().color(palette.text_primary));
                            ui.label(RichText::new("Auto-detect hosts and identities defined in ~/.ssh/config").size(10.5).color(palette.text_muted));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let scan_btn = egui::Button::new(RichText::new("Scan Hosts").size(11.0).color(palette.text_primary))
                                .fill(Color32::from_rgb(26, 29, 40))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 58)))
                                .rounding(Rounding::same(6.0));
                            if ui.add(scan_btn).clicked() {
                                let imported = crate::remote::import_from_ssh_config();
                                if let Some(first) = imported.first() {
                                    state.name = first.name.clone();
                                    state.host = first.host.clone();
                                    state.port_str = first.port.to_string();
                                    state.user = first.user.clone();
                                    state.key_path = first.key_path.clone();
                                    state.test_result = Some(Ok(format!("Detected '{}' ({} hosts in ~/.ssh/config)", first.name, imported.len())));
                                } else {
                                    state.test_result = Some(Err("No hosts found in ~/.ssh/config".to_string()));
                                }
                            }
                        });
                    });
                });

            ui.add_space(10.0);

            // Display Name *
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.label(RichText::new("Display Name").size(12.0).strong().color(palette.text_primary));
                ui.label(RichText::new("*").size(12.0).strong().color(palette.danger));
            });
            ui.add(
                egui::TextEdit::singleline(&mut state.name)
                    .hint_text("e.g. Dev Cloud Server, Staging VPS")
                    .clip_text(true)
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Host & Port row
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.label(RichText::new("Host / IP Address").size(12.0).strong().color(palette.text_primary));
                        ui.label(RichText::new("*").size(12.0).strong().color(palette.danger));
                    });
                    let host_w = (ui.available_width() - 95.0).max(80.0);
                    ui.add_sized(
                        [host_w, 20.0],
                        egui::TextEdit::singleline(&mut state.host)
                            .hint_text("e.g. 192.168.1.100 or vps.example.com")
                            .clip_text(true),
                    );
                });
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.label(RichText::new("Port").size(12.0).strong().color(palette.text_primary));
                        ui.label(RichText::new("*").size(12.0).strong().color(palette.danger));
                    });
                    ui.add(
                        egui::TextEdit::singleline(&mut state.port_str)
                            .hint_text("22")
                            .clip_text(true)
                            .desired_width(75.0),
                    );
                });
            });
            ui.add_space(8.0);

            // SSH Username *
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.label(RichText::new("SSH Username").size(12.0).strong().color(palette.text_primary));
                ui.label(RichText::new("*").size(12.0).strong().color(palette.danger));
            });
            ui.add(
                egui::TextEdit::singleline(&mut state.user)
                    .hint_text("ubuntu")
                    .clip_text(true)
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // SSH Private Key Path (optional)
            ui.label(RichText::new("SSH Private Key Path (optional if using agent/default)").size(12.0).strong().color(palette.text_primary));
            ui.horizontal(|ui| {
                let browse_w = 85.0;
                let input_w = (ui.available_width() - browse_w - ui.spacing().item_spacing.x).max(80.0);
                ui.add_sized(
                    [input_w, 20.0],
                    egui::TextEdit::singleline(&mut state.key_path)
                        .hint_text("e.g. ~/.ssh/id_ed25519 or /home/user/.ssh/id_rsa")
                        .clip_text(true),
                );
                let browse_btn = egui::Button::new(RichText::new("📁 Browse...").size(11.0).color(palette.text_secondary))
                    .fill(Color32::from_rgb(26, 29, 40))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 58)))
                    .rounding(Rounding::same(6.0));
                if ui.add(browse_btn).clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        state.key_path = path.to_string_lossy().to_string();
                    }
                }
            });
            ui.add_space(8.0);

            // SSH Password (optional)
            ui.horizontal(|ui| {
                ui.label(RichText::new("SSH Password (optional if using key/agent)").size(12.0).strong().color(palette.text_primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let eye_label = if state.show_password { "👁 Hide" } else { "👁 Show" };
                    if ui.small_button(eye_label).clicked() {
                        state.show_password = !state.show_password;
                    }
                });
            });
            ui.add(
                egui::TextEdit::singleline(&mut state.password)
                    .password(!state.show_password)
                    .hint_text("Enter SSH password (for password-based authentication)")
                    .clip_text(true)
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Default Remote Directory (optional)
            ui.label(RichText::new("Default Remote Directory (optional)").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.remote_cwd)
                    .hint_text("e.g. /var/www or /home/ubuntu/app")
                    .clip_text(true)
                    .desired_width(ui.available_width()),
            );
            ui.add_space(10.0);

            // Checkboxes: Use local SSH agent authentication & Strict host key check
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.use_agent, RichText::new("Use local SSH agent authentication").size(11.5).color(palette.text_secondary));
                ui.add_space(12.0);
                ui.checkbox(&mut state.strict_host_key, RichText::new("Strict host key check").size(11.5).color(palette.text_secondary));
            });
            ui.add_space(10.0);

            ui.separator();
            ui.add_space(6.0);

            // Footer
            ui.horizontal(|ui| {
                // Test Connection button
                let test_btn = egui::Button::new(RichText::new("⚡ Test Connection").size(11.5).color(Color32::from_rgb(196, 192, 255)))
                    .fill(Color32::from_rgb(26, 29, 42))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 50, 72)))
                    .rounding(Rounding::same(999.0));
                if ui.add(test_btn).clicked() {
                    let port = state.port_str.trim().parse::<u16>().unwrap_or(22);
                    let host_obj = crate::config::SshRemoteHost {
                        id: "test".to_string(),
                        name: state.name.clone(),
                        host: state.host.clone(),
                        port,
                        user: state.user.clone(),
                        key_path: state.key_path.clone(),
                        password: state.password.clone(),
                        remote_cwd: state.remote_cwd.clone(),
                    };
                    state.test_result = Some(crate::remote::test_ssh_connection(&host_obj));
                }

                ui.add_space(6.0);
                if let Some(res) = &state.test_result {
                    match res {
                        Ok(msg) => {
                            ui.label(RichText::new("●").size(9.0).color(palette.success));
                            ui.label(RichText::new(msg).size(11.0).color(palette.success));
                        }
                        Err(err) => {
                            ui.label(RichText::new("●").size(9.0).color(palette.danger));
                            ui.label(RichText::new(err).size(11.0).color(palette.danger));
                        }
                    }
                } else {
                    ui.label(RichText::new("●").size(9.0).color(Color32::from_rgb(0, 210, 160)));
                    ui.label(RichText::new("Ready to register").size(11.0).monospace().color(Color32::from_rgb(120, 126, 142)));
                }

                if state.is_edit {
                    if let Some(id_to_del) = state.editing_id.clone() {
                        let del_btn = egui::Button::new(RichText::new("Delete Host").size(11.5).color(palette.danger))
                            .fill(palette.danger_subtle);
                        if ui.add(del_btn).clicked() {
                            action = ModalAction::DeleteSshHost(id_to_del);
                            state.open = false;
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save_btn = egui::Button::new(RichText::new("Save Host").size(12.0).strong().color(Color32::from_rgb(24, 25, 34)))
                        .fill(Color32::from_rgb(196, 192, 255))
                        .rounding(Rounding::same(8.0))
                        .min_size(egui::vec2(95.0, 28.0));

                    if ui.add(save_btn).clicked() && !state.host.trim().is_empty() {
                        let port = state.port_str.trim().parse::<u16>().unwrap_or(22);
                        let name = if state.name.trim().is_empty() {
                            state.host.trim().to_string()
                        } else {
                            state.name.trim().to_string()
                        };
                        let id = state.editing_id.clone().unwrap_or_else(|| {
                            format!("ssh_{}", uuid_short())
                        });

                        let host_obj = crate::config::SshRemoteHost {
                            id,
                            name,
                            host: state.host.trim().to_string(),
                            port,
                            user: if state.user.trim().is_empty() { "ubuntu".to_string() } else { state.user.trim().to_string() },
                            key_path: state.key_path.trim().to_string(),
                            password: state.password.trim().to_string(),
                            remote_cwd: state.remote_cwd.trim().to_string(),
                        };

                        action = ModalAction::SaveSshHost(host_obj, state.editing_id.clone());
                        state.open = false;
                        is_open = false;
                    }

                    let cancel_btn = egui::Button::new(RichText::new("Cancel").size(12.0).color(palette.text_secondary))
                        .fill(Color32::from_rgb(26, 29, 40))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 58)))
                        .rounding(Rounding::same(6.0))
                        .min_size(egui::vec2(75.0, 28.0));
                    if ui.add(cancel_btn).clicked() {
                        state.open = false;
                        is_open = false;
                    }
                });
            });
        });

    if !is_open {
        state.open = false;
    }
    action
}

fn uuid_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{:x}", d.as_millis() % 0xffffff)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseConfirmChoice {
    None,
    Cancel,
    StopAndExit,
    KeepRunningAndExit,
}

pub fn render_close_confirmation_modal(
    ctx: &Context,
    open: &mut bool,
    running_servers: &[(String, Option<u16>, Option<u32>)],
    palette: &ThemePalette,
) -> CloseConfirmChoice {
    if !*open {
        return CloseConfirmChoice::None;
    }

    let mut choice = CloseConfirmChoice::None;
    let mut is_open = *open;

    let screen = ctx.screen_rect();
    let modal_w = 460.0_f32.min(screen.width() - 40.0).max(320.0);

    let window_id = egui::Id::new("exit_confirmation_modal_dialog");
    let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();
    Window::new("Exit Confirmation")
        .id(window_id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .current_pos(screen.center() + drag_offset)
        .pivot(egui::Align2::CENTER_CENTER)
        .min_width(modal_w)
        .max_width(modal_w)
        .default_height(280.0)
        .frame(
            egui::Frame::none()
                .fill(Color32::from_rgb(17, 19, 27))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                .rounding(Rounding::same(16.0))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 8.0),
                    blur: 24.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(160),
                })
                .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
        )
        .show(ctx, |ui| {
            render_modal_titlebar(ui, "Exit Confirmation", &mut is_open);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                // Header with warning icon
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚠️").size(24.0));
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Active Servers Still Running")
                                .size(16.0)
                                .strong()
                                .color(palette.text_primary),
                        );
                        ui.label(
                            RichText::new(format!(
                                "You have {} active server{} running.",
                                running_servers.len(),
                                if running_servers.len() == 1 { "" } else { "s" }
                            ))
                            .size(12.0)
                            .color(palette.text_muted),
                        );
                    });
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    RichText::new("Would you like to stop all running servers before exiting, or leave them running in the background?")
                        .size(12.5)
                        .color(palette.text_secondary),
                );

                ui.add_space(8.0);

                // Running servers list
                egui::Frame::none()
                    .fill(palette.bg_subtle)
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(100.0)
                            .show(ui, |ui| {
                                for (name, port, pid) in running_servers {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("●").size(10.0).color(palette.success));
                                        ui.label(RichText::new(name).size(12.0).strong().color(palette.text_primary));
                                        if let Some(p) = port {
                                            if *p > 0 {
                                                ui.label(RichText::new(format!(":{}", p)).size(11.0).color(palette.accent));
                                            }
                                        }
                                        if let Some(id) = pid {
                                            ui.label(RichText::new(format!("(PID {})", id)).size(10.5).color(palette.text_muted));
                                        }
                                    });
                                }
                            });
                    });

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(10.0);

                // Action buttons: Cancel, Keep Running & Exit, Stop & Exit
                ui.horizontal(|ui| {
                    if ui.add(
                        egui::Button::new(RichText::new("Cancel").size(12.0).color(palette.text_secondary))
                            .fill(palette.bg_button)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                    ).clicked() {
                        choice = CloseConfirmChoice::Cancel;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn_stop = egui::Button::new(
                            RichText::new("🛑 Stop & Exit").size(12.0).strong().color(Color32::WHITE),
                        )
                        .fill(palette.danger)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(999.0))
                        .min_size(egui::vec2(100.0, 28.0));

                        if ui.add(btn_stop).clicked() {
                            choice = CloseConfirmChoice::StopAndExit;
                        }

                        let btn_keep = egui::Button::new(
                            RichText::new("Keep Running & Exit").size(12.0).color(palette.text_primary),
                        )
                        .fill(palette.bg_button)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(999.0))
                        .min_size(egui::vec2(130.0, 28.0));

                        if ui.add(btn_keep).on_hover_text("Leave processes active in the background. Server Launcher will reconnect to them when reopened.").clicked() {
                            choice = CloseConfirmChoice::KeepRunningAndExit;
                        }
                    });
                });
            });
        });

    if !is_open && choice == CloseConfirmChoice::None {
        choice = CloseConfirmChoice::Cancel;
    }

    if choice != CloseConfirmChoice::None {
        *open = false;
    }

    choice
}

// ═════════════════════════════════════════════════════════════════════════════
// SCRIPT MODAL (Create / Edit Custom Script)
// ═════════════════════════════════════════════════════════════════════════════
pub fn render_script_modal(
    ctx: &Context,
    state: &mut ScriptModalState,
    palette: &ThemePalette,
) -> Option<CustomScript> {
    if !state.open {
        return None;
    }

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.open = false;
        return None;
    }

    let mut result = None;
    let title = if state.is_edit { "✏ Edit Custom Script" } else { "📜 New Custom Script" };

    let screen = ctx.screen_rect();
    let modal_w = 540.0_f32.min(screen.width() - 40.0).max(360.0);
    let max_h = (screen.height() - 40.0).max(400.0);

    let mut is_open = state.open;
    let window_id = egui::Id::new("script_modal_dialog");
    let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();
    Window::new(title)
        .id(window_id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .current_pos(screen.center() + drag_offset)
        .pivot(egui::Align2::CENTER_CENTER)
        .min_width(modal_w)
        .max_width(modal_w)
        .default_height(480.0)
        .max_height(max_h)
        .frame(
            egui::Frame::none()
                .fill(Color32::from_rgb(17, 19, 27))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                .rounding(Rounding::same(16.0))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 8.0),
                    blur: 24.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(160),
                })
                .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
        )
        .show(ctx, |ui| {
            render_modal_titlebar(ui, title, &mut is_open);
            ui.add_space(8.0);
            ui.add_space(4.0);

            // Error display if any
            if let Some(err) = &state.error {
                ui.label(RichText::new(err).size(11.5).color(palette.danger));
                ui.add_space(4.0);
            }

            // Script Name
            ui.label(RichText::new("Script Name:").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.name)
                    .hint_text("e.g. Health Check, DB Backup, Build & Clean")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Shell & CWD row
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Shell / Interpreter:").size(12.0).strong().color(palette.text_primary));
                    egui::ComboBox::from_id_salt("script_shell_combo")
                        .selected_text(&state.shell)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut state.shell, "bash".to_string(), "bash");
                            ui.selectable_value(&mut state.shell, "sh".to_string(), "sh");
                            ui.selectable_value(&mut state.shell, "python3".to_string(), "python3");
                            ui.selectable_value(&mut state.shell, "zsh".to_string(), "zsh");
                        });
                });

                ui.add_space(12.0);

                ui.vertical(|ui| {
                    ui.label(RichText::new("Working Directory (optional):").size(12.0).strong().color(palette.text_primary));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut state.cwd)
                                .hint_text("Default (current / home)")
                                .desired_width(ui.available_width() - 80.0),
                        );
                        if ui.button("Browse...").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                state.cwd = path.to_string_lossy().to_string();
                            }
                        }
                    });
                });
            });
            ui.add_space(8.0);

            // Startup Autostart Toggle
            ui.checkbox(
                &mut state.autostart,
                RichText::new("⚡ Run automatically at startup / connection")
                    .size(12.0)
                    .color(palette.accent)
                    .strong(),
            );
            ui.label(
                RichText::new("If enabled, this script will be triggered immediately when the launcher launches or connects to this host.")
                    .size(11.0)
                    .color(palette.text_muted),
            );
            ui.add_space(10.0);

            // Script Body
            ui.label(RichText::new("Script Content:").size(12.0).strong().color(palette.text_primary));
            let text_height = (ui.available_height() - 50.0).max(160.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut state.script_body)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(10)
                        .desired_width(ui.available_width())
                        .min_size(egui::vec2(0.0, text_height)),
                );
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(6.0);

            // Action Buttons
            ui.horizontal(|ui| {
                let btn_cancel = egui::Button::new(RichText::new("Cancel").size(12.0).color(palette.text_secondary))
                    .fill(Color32::from_rgb(26, 29, 40))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 58)))
                    .rounding(Rounding::same(6.0))
                    .min_size(egui::vec2(75.0, 28.0));
                if ui.add(btn_cancel).clicked() {
                    state.open = false;
                    is_open = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_save = egui::Button::new(
                        RichText::new("💾 Save Script").size(12.0).strong().color(Color32::WHITE),
                    )
                    .fill(palette.accent)
                    .rounding(Rounding::same(5.0))
                    .min_size(egui::vec2(100.0, 28.0));

                    if ui.add(btn_save).clicked() {
                        if state.name.trim().is_empty() {
                            state.error = Some("Script name cannot be empty".to_string());
                        } else if state.script_body.trim().is_empty() {
                            state.error = Some("Script body cannot be empty".to_string());
                        } else {
                            result = Some(CustomScript {
                                id: state.id.clone(),
                                name: state.name.trim().to_string(),
                                script: state.script_body.clone(),
                                shell: state.shell.trim().to_string(),
                                cwd: state.cwd.trim().to_string(),
                                autostart: state.autostart,
                                host_id: state.host_id.clone(),
                            });
                            state.open = false;
                            is_open = false;
                        }
                    }
                });
            });
        });

    if !is_open {
        state.open = false;
    }

    result
}

// ═════════════════════════════════════════════════════════════════════════════
// SAVED COMMAND MODAL (Create / Edit Custom Command)
// ═════════════════════════════════════════════════════════════════════════════
pub fn render_command_modal(
    ctx: &Context,
    state: &mut CommandModalState,
    palette: &ThemePalette,
) -> Option<SavedCommand> {
    if !state.open {
        return None;
    }

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.open = false;
        return None;
    }

    let mut result = None;
    let title = if state.is_edit { "✏ Edit Saved Command" } else { "⚡ Save Custom Command" };

    let screen = ctx.screen_rect();
    let modal_w = 480.0_f32.min(screen.width() - 40.0).max(340.0);
    let max_h = (screen.height() - 40.0).max(340.0);

    let mut is_open = state.open;
    let window_id = egui::Id::new("command_modal_dialog");
    let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();
    Window::new(title)
        .id(window_id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .current_pos(screen.center() + drag_offset)
        .pivot(egui::Align2::CENTER_CENTER)
        .min_width(modal_w)
        .max_width(modal_w)
        .default_height(320.0)
        .max_height(max_h)
        .frame(
            egui::Frame::none()
                .fill(Color32::from_rgb(17, 19, 27))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                .rounding(Rounding::same(16.0))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 8.0),
                    blur: 24.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(160),
                })
                .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
        )
        .show(ctx, |ui| {
            render_modal_titlebar(ui, title, &mut is_open);
            ui.add_space(8.0);
            ui.add_space(4.0);

            if let Some(err) = &state.error {
                ui.label(RichText::new(err).size(11.5).color(palette.danger));
                ui.add_space(4.0);
            }

            // Command Name
            ui.label(RichText::new("Name / Label:").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.name)
                    .hint_text("e.g. Git Status & Log, Prune Docker, Disk Free")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Category & Host
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Category:").size(12.0).strong().color(palette.text_primary));
                    ui.add(
                        egui::TextEdit::singleline(&mut state.category)
                            .hint_text("e.g. Git, Docker, System, Deploy")
                            .desired_width(200.0),
                    );
                });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.label(RichText::new("Scope:").size(12.0).strong().color(palette.text_primary));
                    let scope_str = if let Some(h) = &state.host_id {
                        format!("Host: {}", h)
                    } else {
                        "Local PC".to_string()
                    };
                    ui.label(RichText::new(scope_str).size(12.0).color(palette.text_muted));
                });
            });
            ui.add_space(8.0);

            // Command String
            ui.label(RichText::new("Command to Execute:").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::multiline(&mut state.command)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(3)
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Description
            ui.label(RichText::new("Description (optional):").size(12.0).color(palette.text_muted));
            ui.add(
                egui::TextEdit::singleline(&mut state.description)
                    .hint_text("Short note on what this command does")
                    .desired_width(ui.available_width()),
            );

            // Run with external terminal option
            ui.add_space(8.0);
            let is_ext_term = state.run_in_terminal;
            ui.checkbox(
                &mut state.run_in_terminal,
                RichText::new("Run with external terminal (for interactive tools like top, htop, nano)")
                    .size(11.5)
                    .color(if is_ext_term { palette.accent } else { palette.text_secondary }),
            ).on_hover_text("Launch an interactive external desktop terminal window when executing this command (required for full-screen TUI tools like top, htop, vim)");

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(6.0);

            // Action Buttons
            ui.horizontal(|ui| {
                let btn_cancel = egui::Button::new(RichText::new("Cancel").size(12.0).color(palette.text_secondary))
                    .fill(Color32::from_rgb(26, 29, 40))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 58)))
                    .rounding(Rounding::same(6.0))
                    .min_size(egui::vec2(75.0, 28.0));
                if ui.add(btn_cancel).clicked() {
                    state.open = false;
                    is_open = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_save = egui::Button::new(
                        RichText::new("💾 Save Command").size(12.0).strong().color(Color32::WHITE),
                    )
                    .fill(palette.accent)
                    .rounding(Rounding::same(5.0))
                    .min_size(egui::vec2(100.0, 28.0));

                    if ui.add(btn_save).clicked() {
                        if state.name.trim().is_empty() {
                            state.error = Some("Command name cannot be empty".to_string());
                        } else if state.command.trim().is_empty() {
                            state.error = Some("Command string cannot be empty".to_string());
                        } else {
                            result = Some(SavedCommand {
                                id: state.id.clone(),
                                name: state.name.trim().to_string(),
                                command: state.command.trim().to_string(),
                                description: state.description.trim().to_string(),
                                category: if state.category.trim().is_empty() {
                                    "General".to_string()
                                } else {
                                    state.category.trim().to_string()
                                },
                                host_id: state.host_id.clone(),
                                run_in_terminal: state.run_in_terminal,
                            });
                            state.open = false;
                            is_open = false;
                        }
                    }
                });
            });
        });

    if !is_open {
        state.open = false;
    }

    result
}

// ═════════════════════════════════════════════════════════════════════════════
// SYSTEMD SERVICE CREATION MODAL
// ═════════════════════════════════════════════════════════════════════════════
#[derive(Debug, Clone, Default)]
pub struct NewServiceModalState {
    pub open: bool,
    pub def: NewServiceDefinition,
    pub error: Option<String>,
}

impl NewServiceModalState {
    pub fn open(&mut self) {
        self.open = true;
        self.def = NewServiceDefinition::default();
        self.error = None;
    }
}

pub fn render_new_service_modal(
    ctx: &Context,
    state: &mut NewServiceModalState,
    palette: &ThemePalette,
) -> Option<NewServiceDefinition> {
    if !state.open {
        return None;
    }

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.open = false;
        return None;
    }

    let mut result = None;
    let screen = ctx.screen_rect();
    let modal_w = 540.0_f32.min(screen.width() - 40.0).max(360.0);
    let max_h = (screen.height() - 40.0).max(400.0);
    let mut is_open = state.open;

    let window_id = egui::Id::new("new_systemd_service_modal_dialog");
    let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();
    Window::new("Create New SystemMD Service")
        .id(window_id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .current_pos(screen.center() + drag_offset)
        .pivot(egui::Align2::CENTER_CENTER)
        .min_width(modal_w)
        .max_width(modal_w)
        .default_height(480.0)
        .max_height(max_h)
        .frame(
            egui::Frame::none()
                .fill(Color32::from_rgb(17, 19, 27))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                .rounding(Rounding::same(16.0))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 8.0),
                    blur: 24.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(160),
                })
                .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
        )
        .show(ctx, |ui| {
            render_modal_titlebar(ui, "Create New SystemMD Service", &mut is_open);
            ui.add_space(8.0);
            ui.add_space(4.0);

            if let Some(err) = &state.error {
                ui.label(RichText::new(err).size(11.5).color(palette.danger));
                ui.add_space(4.0);
            }

            // Unit Name & Scope
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Service Unit Name:").size(12.0).strong().color(palette.text_primary));
                    ui.add(
                        egui::TextEdit::singleline(&mut state.def.name)
                            .hint_text("e.g. my-app.service")
                            .desired_width(280.0),
                    );
                });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.label(RichText::new("Scope:").size(12.0).strong().color(palette.text_primary));
                    ui.checkbox(&mut state.def.is_user, "User Service (--user)");
                });
            });
            ui.add_space(8.0);

            // Description
            ui.label(RichText::new("Description:").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.def.description)
                    .hint_text("e.g. My Background Web Service")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // ExecStart
            ui.label(RichText::new("ExecStart (Executable & Arguments):").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.def.exec_start)
                    .hint_text("e.g. /usr/bin/python3 /home/user/app/server.py")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Working Directory & Restart
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Working Directory:").size(12.0).strong().color(palette.text_primary));
                    ui.add(
                        egui::TextEdit::singleline(&mut state.def.working_dir)
                            .hint_text("e.g. /home/user/app")
                            .desired_width(ui.available_width() - 140.0),
                    );
                });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.label(RichText::new("Restart Policy:").size(12.0).strong().color(palette.text_primary));
                    egui::ComboBox::from_id_salt("service_restart_combo")
                        .selected_text(&state.def.restart)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut state.def.restart, "always".to_string(), "always");
                            ui.selectable_value(&mut state.def.restart, "on-failure".to_string(), "on-failure");
                            ui.selectable_value(&mut state.def.restart, "no".to_string(), "no");
                        });
                });
            });
            ui.add_space(8.0);

            // Environment
            ui.label(RichText::new("Environment Variables (e.g. PORT=8080 NODE_ENV=production):").size(12.0).color(palette.text_muted));
            ui.add(
                egui::TextEdit::singleline(&mut state.def.environment)
                    .hint_text("PORT=8080")
                    .desired_width(ui.available_width()),
            );

            ui.add_space(8.0);

            // Generated Preview
            ui.collapsing("🔍 Preview Generated Unit File", |ui| {
                let preview = state.def.generate_service_content();
                ui.add(
                    egui::TextEdit::multiline(&mut preview.as_str())
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(6)
                        .desired_width(ui.available_width()),
                );
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(6.0);

            // Action Buttons
            ui.horizontal(|ui| {
                let btn_cancel = egui::Button::new(RichText::new("Cancel").size(12.0).color(palette.text_secondary))
                    .fill(Color32::from_rgb(26, 29, 40))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 58)))
                    .rounding(Rounding::same(6.0))
                    .min_size(egui::vec2(75.0, 28.0));
                if ui.add(btn_cancel).clicked() {
                    state.open = false;
                    is_open = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_create = egui::Button::new(
                        RichText::new("🚀 Create & Enable Service").size(12.0).strong().color(Color32::WHITE),
                    )
                    .fill(palette.accent)
                    .rounding(Rounding::same(5.0))
                    .min_size(egui::vec2(150.0, 28.0));

                    if ui.add(btn_create).clicked() {
                        if state.def.name.trim().is_empty() {
                            state.error = Some("Service name cannot be empty".to_string());
                        } else if state.def.exec_start.trim().is_empty() {
                            state.error = Some("ExecStart cannot be empty".to_string());
                        } else {
                            result = Some(state.def.clone());
                            state.open = false;
                            is_open = false;
                        }
                    }
                });
            });
        });

    if !is_open {
        state.open = false;
    }

    result
}

#[derive(Debug, Clone, Default)]
pub struct RemoteStartServerModalState {
    pub open: bool,
    pub host_id: String,
    pub host_name: String,
    pub name: String,
    pub port: String,
    pub command: String,
    pub cwd: String,
    pub error: Option<String>,
}

impl RemoteStartServerModalState {
    pub fn open_for_host(&mut self, host: &crate::config::SshRemoteHost) {
        self.open = true;
        self.host_id = host.id.clone();
        self.host_name = host.name.clone();
        self.name.clear();
        self.port.clear();
        self.command.clear();
        self.cwd = host.remote_cwd.clone();
        self.error = None;
    }
}

#[derive(Debug, Clone)]
pub struct RemoteStartServerRequest {
    pub host_id: String,
    pub name: String,
    pub port: Option<u16>,
    pub command: String,
    pub cwd: String,
}

pub fn render_remote_start_server_modal(
    ctx: &Context,
    state: &mut RemoteStartServerModalState,
    palette: &ThemePalette,
) -> Option<RemoteStartServerRequest> {
    if !state.open {
        return None;
    }

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.open = false;
        return None;
    }

    let mut result = None;
    let screen = ctx.screen_rect();
    let modal_w = 480.0_f32.min(screen.width() - 40.0).max(360.0);
    let mut is_open = state.open;

    let window_id = egui::Id::new("remote_start_server_modal_dialog");
    let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();
    Window::new("Start Remote Server")
        .id(window_id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .current_pos(screen.center() + drag_offset)
        .pivot(egui::Align2::CENTER_CENTER)
        .min_width(modal_w)
        .max_width(modal_w)
        .frame(
            egui::Frame::none()
                .fill(Color32::from_rgb(17, 19, 27))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                .rounding(Rounding::same(16.0))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 8.0),
                    blur: 24.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(160),
                })
                .inner_margin(egui::Margin {
                    left: 20.0,
                    right: 20.0,
                    top: 14.0,
                    bottom: 18.0,
                }),
        )
        .show(ctx, |ui| {
            render_modal_titlebar(ui, "Start Remote Server", &mut is_open);
            ui.add_space(10.0);

            // Subtitle: Target host info
            ui.horizontal(|ui| {
                ui.label(RichText::new("Target Host:").size(11.0).color(palette.text_muted));
                egui::Frame::none()
                    .fill(palette.accent_subtle)
                    .rounding(Rounding::same(999.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(&state.host_name).size(11.0).strong().color(palette.accent));
                    });
            });
            ui.add_space(8.0);

            if let Some(ref err) = state.error {
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(239, 68, 68, 40))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(format!("⚠️ {}", err)).size(11.0).color(Color32::from_rgb(252, 165, 165)));
                    });
                ui.add_space(8.0);
            }

            // Server Name
            ui.label(RichText::new("Server / Service Name:").size(11.5).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.name)
                    .hint_text("e.g. web-app, node-backend, nginx")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Port (optional)
            ui.label(RichText::new("Port (optional):").size(11.5).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.port)
                    .hint_text("e.g. 8080 or 3000")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Command
            ui.label(RichText::new("Launch Command:").size(11.5).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.command)
                    .hint_text("e.g. python3 -m http.server 8080 or npm run dev")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Working directory
            ui.label(RichText::new("Working Directory:").size(11.5).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.cwd)
                    .hint_text("e.g. /var/www or ~/projects/app")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(14.0);

            // Buttons
            ui.horizontal(|ui| {
                let btn_cancel = egui::Button::new(RichText::new("Cancel").size(12.0).color(palette.text_secondary))
                    .fill(Color32::from_rgb(26, 29, 40))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 58)))
                    .rounding(Rounding::same(6.0))
                    .min_size(egui::vec2(75.0, 28.0));
                if ui.add(btn_cancel).clicked() {
                    state.open = false;
                    is_open = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_start = egui::Button::new(
                        RichText::new("▶ Start Server").size(12.0).strong().color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(0x05, 0x96, 0x69))
                    .rounding(Rounding::same(999.0))
                    .min_size(egui::vec2(120.0, 28.0));

                    if ui.add(btn_start).clicked() {
                        let cmd_trimmed = state.command.trim();
                        if cmd_trimmed.is_empty() {
                            state.error = Some("Launch Command cannot be empty".to_string());
                        } else {
                            let name = if state.name.trim().is_empty() {
                                if let Ok(p) = state.port.trim().parse::<u16>() {
                                    format!("port_{}", p)
                                } else {
                                    cmd_trimmed.split_whitespace().next().unwrap_or("remote_server").to_string()
                                }
                            } else {
                                state.name.trim().to_string()
                            };

                            let parsed_port = state.port.trim().parse::<u16>().ok();

                            result = Some(RemoteStartServerRequest {
                                host_id: state.host_id.clone(),
                                name,
                                port: parsed_port,
                                command: cmd_trimmed.to_string(),
                                cwd: state.cwd.trim().to_string(),
                            });
                            state.open = false;
                            is_open = false;
                        }
                    }
                });
            });
        });

    if !is_open {
        state.open = false;
    }

    result
}

// ═════════════════════════════════════════════════════════════════════════════
// SUDO / ROOT PASSWORD MODAL (Ask for sudo password when scripts require it)
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Default)]
pub struct SudoModalState {
    pub open: bool,
    pub script: Option<CustomScript>,
    pub host_id: Option<String>,
    pub host_name: Option<String>,
    pub password: String,
    pub show_password: bool,
    pub error: Option<String>,
    pub auto_focus_requested: bool,
}

impl SudoModalState {
    pub fn open_for_script(&mut self, script: CustomScript, host: Option<&SshRemoteHost>) {
        self.open = true;
        self.script = Some(script);
        self.host_id = host.map(|h| h.id.clone());
        self.host_name = host.map(|h| format!("{}@{}:{}", h.user, h.host, h.port));
        self.password.clear();
        self.show_password = false;
        self.error = None;
        self.auto_focus_requested = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.script = None;
        self.host_id = None;
        self.host_name = None;
        self.password.clear();
        self.error = None;
        self.auto_focus_requested = false;
    }
}

pub fn render_sudo_modal(
    ctx: &Context,
    state: &mut SudoModalState,
    palette: &ThemePalette,
) -> Option<(CustomScript, Option<String>, String)> {
    if !state.open {
        return None;
    }

    let mut result = None;
    let title = "🔒 Superuser Authentication";

    let screen = ctx.screen_rect();
    let modal_w = 460.0_f32.min(screen.width() - 40.0).max(340.0);

    let mut is_open = state.open;
    let window_id = egui::Id::new("sudo_password_modal_dialog");
    let drag_offset = ctx
        .data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset")))
        .unwrap_or_default();

    let mut submit_requested = false;

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        is_open = false;
    }

    Window::new(title)
        .id(window_id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .current_pos(screen.center() + drag_offset)
        .pivot(egui::Align2::CENTER_CENTER)
        .min_width(modal_w)
        .max_width(modal_w)
        .frame(
            egui::Frame::none()
                .fill(Color32::from_rgb(17, 19, 27))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                .rounding(Rounding::same(16.0))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 8.0),
                    blur: 24.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(180),
                })
                .inner_margin(egui::Margin::symmetric(16.0, 14.0)),
        )
        .show(ctx, |ui| {
            render_modal_titlebar(ui, title, &mut is_open);
            ui.add_space(10.0);

            let script_name = state.script.as_ref().map(|s| s.name.as_str()).unwrap_or("Script");
            let target_desc = if let Some(ref h) = state.host_name {
                format!("Target Host: {}", h)
            } else {
                "Target: Local Machine".to_string()
            };

            egui::Frame::none()
                .fill(Color32::from_rgb(24, 27, 39))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 43, 62)))
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🛡").size(22.0));
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(format!("'{}' requires sudo / root privileges", script_name))
                                    .size(12.5)
                                    .strong()
                                    .color(palette.text_primary),
                            );
                            ui.label(
                                RichText::new(target_desc)
                                    .size(11.0)
                                    .color(Color32::from_rgb(99, 102, 241)),
                            );
                        });
                    });
                });

            ui.add_space(10.0);

            if let Some(err) = &state.error {
                egui::Frame::none()
                    .fill(Color32::from_rgb(45, 18, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(150, 30, 45)))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("✘").size(12.0).color(palette.danger));
                            ui.label(RichText::new(err).size(11.5).color(palette.danger));
                        });
                    });
                ui.add_space(8.0);
            }

            ui.label(
                RichText::new("Enter Sudo Password:")
                    .size(12.0)
                    .strong()
                    .color(palette.text_primary),
            );
            ui.add_space(4.0);

            let text_edit_response = ui.horizontal(|ui| {
                let avail_w = (ui.available_width() - 36.0).max(100.0);
                let text_edit = egui::TextEdit::singleline(&mut state.password)
                    .password(!state.show_password)
                    .hint_text("Enter root/sudo password...")
                    .desired_width(avail_w);
                let resp = ui.add(text_edit);
                if state.auto_focus_requested {
                    resp.request_focus();
                    state.auto_focus_requested = false;
                }

                let eye_btn = egui::Button::new(
                    RichText::new(if state.show_password { "👁" } else { "🔒" }).size(12.0),
                )
                .fill(Color32::from_rgb(30, 34, 48))
                .rounding(Rounding::same(6.0));
                if ui.add(eye_btn).on_hover_text("Toggle password visibility").clicked() {
                    state.show_password = !state.show_password;
                }

                resp
            }).inner;

            if text_edit_response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                submit_requested = true;
            }

            ui.add_space(6.0);
            ui.label(
                RichText::new("🔒 Password is securely provided via askpass helper and not saved.")
                    .size(10.5)
                    .color(palette.text_muted),
            );

            ui.add_space(14.0);

            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_submit = egui::Button::new(
                        RichText::new("▶ Authenticate & Run")
                            .size(12.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(16, 185, 129))
                    .rounding(Rounding::same(8.0))
                    .min_size(egui::vec2(150.0, 30.0));

                    if ui.add(btn_submit).clicked() {
                        submit_requested = true;
                    }

                    let btn_cancel = egui::Button::new(
                        RichText::new("Cancel")
                            .size(12.0)
                            .color(palette.text_muted),
                    )
                    .fill(Color32::from_rgb(30, 34, 48))
                    .rounding(Rounding::same(8.0))
                    .min_size(egui::vec2(80.0, 30.0));

                    if ui.add(btn_cancel).clicked() {
                        is_open = false;
                        state.open = false;
                    }
                });
            });
        });

    if submit_requested {
        if state.password.trim().is_empty() {
            state.error = Some("Password cannot be empty.".to_string());
        } else if let Some(script) = state.script.clone() {
            if state.host_id.is_none() {
                // Local script: validate password locally
                match crate::scripts::validate_local_sudo_password(&state.password) {
                    Ok(()) => {
                        result = Some((script, None, state.password.clone()));
                        state.open = false;
                        is_open = false;
                    }
                    Err(err) => {
                        state.error = Some(err);
                    }
                }
            } else {
                // Remote script: pass password directly to remote SSH execution
                result = Some((script, state.host_id.clone(), state.password.clone()));
                state.open = false;
                is_open = false;
            }
        }
    }

    if !is_open {
        state.close();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_json_snippet_user_mcp_format() {
        let mut modal = AddEditModalState::default();
        let snippet = r#"{
  "servers": {
    "stitch": {
      "type": "http",
      "url": "https://stitch.googleapis.com/mcp",
      "headers": {
        "X-Goog-Api-Key": "YOUR_API_KEY"
      }
    }
  }
}"#;
        let res = modal.apply_json_snippet(snippet);
        assert!(res.is_ok(), "apply_json_snippet should succeed");
        assert_eq!(modal.key, "stitch");
        assert_eq!(modal.name, "Stitch");
        assert_eq!(modal.server_type, "http");
        assert_eq!(modal.url, "https://stitch.googleapis.com/mcp");
        assert_eq!(modal.group, "MCP");
        assert_eq!(modal.headers.len(), 1);
        assert_eq!(modal.headers[0].0, "X-Goog-Api-Key");
        assert_eq!(
            modal.headers[0].1,
            "YOUR_API_KEY"
        );
        assert!(modal.command.contains("curl -i"));
        assert!(modal.command.contains("X-Goog-Api-Key"));

        let cfg = modal.to_server_config().expect("Should convert to ServerConfig");
        assert_eq!(cfg.key, "stitch");
        assert_eq!(cfg.server_type.as_deref(), Some("http"));
        assert_eq!(cfg.url.as_deref(), Some("https://stitch.googleapis.com/mcp"));
        assert_eq!(cfg.group, "MCP");
    }

    #[test]
    fn test_sudo_modal_state() {
        let mut modal = SudoModalState::default();
        let script = CustomScript {
            id: "s1".to_string(),
            name: "Test Sudo".to_string(),
            script: "sudo apt update".to_string(),
            shell: "bash".to_string(),
            cwd: "".to_string(),
            autostart: false,
            host_id: None,
        };

        modal.open_for_script(script.clone(), None);
        assert!(modal.open);
        assert_eq!(modal.script.as_ref().map(|s| s.name.as_str()), Some("Test Sudo"));
        assert_eq!(modal.host_id, None);
        assert!(modal.password.is_empty());
        assert!(modal.auto_focus_requested);

        modal.close();
        assert!(!modal.open);
        assert!(modal.script.is_none());
    }

    #[test]
    fn test_ssh_host_modal_state_close() {
        let mut modal = SshHostModalState::default();
        modal.open = true;
        modal.name = "My SSH".to_string();
        modal.open = false;
        assert!(!modal.open);
    }

    #[test]
    fn test_server_templates_catalog() {
        assert!(
            SERVER_TEMPLATES.len() >= 30,
            "Template catalog should have at least 30 templates, found {}",
            SERVER_TEMPLATES.len()
        );

        let names: Vec<&str> = SERVER_TEMPLATES.iter().map(|t| t.name).collect();
        assert!(
            names.iter().any(|n| n.contains("Firebase")),
            "Catalog must contain Firebase template"
        );
        assert!(
            names.iter().any(|n| n.contains("PostgreSQL")),
            "Catalog must contain PostgreSQL template"
        );
        assert!(
            names.iter().any(|n| n.contains("MySQL")),
            "Catalog must contain MySQL template"
        );
        assert!(
            names.iter().any(|n| n.contains("MariaDB")),
            "Catalog must contain MariaDB template"
        );

        for (idx, tpl) in SERVER_TEMPLATES.iter().enumerate() {
            assert!(!tpl.name.trim().is_empty(), "Template name cannot be empty at {}", idx);
            assert!(!tpl.command.trim().is_empty(), "Template command cannot be empty for {}", tpl.name);
            assert!(!tpl.group.trim().is_empty(), "Template group cannot be empty for {}", tpl.name);

            let mut modal = AddEditModalState::default();
            modal.open_new();
            modal.apply_template(idx + 1);

            assert_eq!(modal.name, tpl.name);
            assert_eq!(modal.command, tpl.command);
            assert_eq!(modal.stop_command, tpl.stop_command);
            assert_eq!(modal.port_str, tpl.port.to_string());
            assert_eq!(modal.group, tpl.group);
            assert!(!modal.key.is_empty(), "Key must not be empty after template apply: {}", tpl.name);

            let config_res = modal.to_server_config();
            assert!(
                config_res.is_ok(),
                "Template {} must convert to a valid ServerConfig: {:?}",
                tpl.name,
                config_res.err()
            );
        }
    }
}


