//! The operating system's address book, read for recipient suggestions. Alice only reads it; the
//! page sends the result to the daemon (`PUT /v1/contacts/system`).

use alice_core::ImportedContact;

/// Every email address in the system address book, one entry per address, after asking the user
/// for access the first time. Runs off the main thread: the permission prompt and a large address
/// book both take a while.
#[tauri::command]
pub async fn read_system_contacts(app: tauri::AppHandle) -> Result<Vec<ImportedContact>, String> {
    let locale = crate::desktop::locale(&app);
    tauri::async_runtime::spawn_blocking(move || platform::read(locale))
        .await
        .map_err(|e| e.to_string())?
}

/// The name shown for a contact: given and family name, or the organization for a company card.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn display_name(given: &str, family: &str, organization: &str) -> String {
    let person = [given.trim(), family.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if person.is_empty() {
        organization.trim().into()
    } else {
        person
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::display_name;
    use alice_core::{ImportedContact, i18n::Locale};
    use block2::RcBlock;
    use objc2::{AnyThread, rc::Retained, runtime::Bool, runtime::ProtocolObject};
    use objc2_contacts::{
        CNAuthorizationStatus, CNContact, CNContactEmailAddressesKey, CNContactFamilyNameKey,
        CNContactFetchRequest, CNContactGivenNameKey, CNContactOrganizationNameKey, CNContactStore,
        CNEntityType, CNKeyDescriptor,
    };
    use objc2_foundation::{NSArray, NSBundle, NSError, NSString};
    use std::{cell::RefCell, ptr::NonNull, sync::mpsc};

    pub fn read(locale: Locale) -> Result<Vec<ImportedContact>, String> {
        // macOS ends a process that touches Contacts without a usage description. The bundled app and
        // `tauri dev` builds embed Info.plist (which has one); refuse anything else instead of crashing.
        if NSBundle::mainBundle()
            .objectForInfoDictionaryKey(&NSString::from_str("NSContactsUsageDescription"))
            .is_none()
        {
            return Err(alice_core::t!(locale, "desktop.contacts.build_unsupported"));
        }
        let store = unsafe { CNContactStore::new() };
        let status =
            unsafe { CNContactStore::authorizationStatusForEntityType(CNEntityType::Contacts) };
        if status == CNAuthorizationStatus::Denied || status == CNAuthorizationStatus::Restricted {
            return Err(alice_core::t!(locale, "desktop.contacts.denied"));
        }
        // The completion handler runs on an arbitrary queue; this blocking thread waits for it.
        let (answered, answer) = mpsc::channel();
        let done = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            let _ = answered.send(granted.as_bool());
        });
        unsafe {
            store.requestAccessForEntityType_completionHandler(CNEntityType::Contacts, &done)
        };
        if !answer.recv().unwrap_or(false) {
            return Err(alice_core::t!(locale, "desktop.contacts.denied"));
        }
        let keys: [&NSString; 4] = unsafe {
            [
                CNContactGivenNameKey,
                CNContactFamilyNameKey,
                CNContactOrganizationNameKey,
                CNContactEmailAddressesKey,
            ]
        };
        let keys: Vec<&ProtocolObject<dyn CNKeyDescriptor>> =
            keys.into_iter().map(ProtocolObject::from_ref).collect();
        let request = unsafe {
            CNContactFetchRequest::initWithKeysToFetch(
                CNContactFetchRequest::alloc(),
                &NSArray::from_slice(&keys),
            )
        };
        let contacts = RefCell::new(Vec::new());
        let each = RcBlock::new(|contact: NonNull<CNContact>, _stop: NonNull<Bool>| {
            // SAFETY: the store passes a valid contact for the duration of the call.
            let contact = unsafe { contact.as_ref() };
            let (given, family, organization, emails) = unsafe {
                (
                    contact.givenName(),
                    contact.familyName(),
                    contact.organizationName(),
                    contact.emailAddresses(),
                )
            };
            let name = display_name(
                &given.to_string(),
                &family.to_string(),
                &organization.to_string(),
            );
            for labeled in emails.to_vec() {
                let email: Retained<NSString> = unsafe { labeled.value() };
                let email = email.to_string().trim().to_string();
                if email.contains('@') {
                    contacts.borrow_mut().push(ImportedContact {
                        name: name.clone(),
                        email,
                    });
                }
            }
        });
        let mut error: Option<Retained<NSError>> = None;
        let read = unsafe {
            store.enumerateContactsWithFetchRequest_error_usingBlock(
                &request,
                Some(&mut error),
                &each,
            )
        };
        drop(each);
        if !read {
            return Err(error.map_or_else(
                || alice_core::t!(locale, "desktop.contacts.read_failed"),
                |error| {
                    alice_core::t!(
                        locale,
                        "desktop.contacts.read_error",
                        error = error.localizedDescription().to_string()
                    )
                },
            ));
        }
        Ok(contacts.into_inner())
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use alice_core::{ImportedContact, i18n::Locale};

    pub fn read(locale: Locale) -> Result<Vec<ImportedContact>, String> {
        Err(alice_core::t!(
            locale,
            "desktop.contacts.platform_unsupported"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::display_name;

    #[test]
    fn people_are_named_by_their_names_and_companies_by_their_organization() {
        assert_eq!(display_name(" Maya ", "Patel", "Form Studio"), "Maya Patel");
        assert_eq!(display_name("", "Patel", ""), "Patel");
        assert_eq!(display_name("", " ", " Form Studio "), "Form Studio");
        assert_eq!(display_name("", "", ""), "");
    }
}
