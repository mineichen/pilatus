use pilatus::Recipes;

pub(super) struct RecipeSelector {
    base_url: String,
    recipes: Option<Recipes>,
}

impl RecipeSelector {
    pub fn from_base(base_url: String) -> Self {
        Self {
            base_url,
            recipes: None,
        }
    }
    pub(super) fn select_ui(&mut self, _ui: &mut egui::Ui) -> Option<Recipes> {
        let list_request = ehttp::Request::get(&self.base_url);
        ehttp::fetch(
            list_request,
            move |result: ehttp::Result<ehttp::Response>| {
                println!("Status code: {:?}", result.unwrap().status);
            },
        );
        None
    }
}
